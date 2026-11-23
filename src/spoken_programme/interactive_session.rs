//! Local, in-memory interaction state driven by confirmed media observations.
use super::playback::Playback;
use super::{Graph, Node, Selection, admit_checked_selection, validate};
use crate::scene_delivery::InputFiles;
use crate::spoken_programme::checker::CheckedPackage;
use anyhow::{Result, anyhow, ensure};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChoiceKey {
    Next,
    Previous,
    Activate,
}

pub struct Session<'a> {
    graph: &'a Graph,
    bindings: BTreeMap<(&'a str, &'a str), Selection<'a>>,
    language: &'a str,
    node: Option<&'a Node>,
    visit: u64,
    frame: Option<u64>,
    paused: bool,
    focus: usize,
    playback: Playback,
    repeats: BTreeMap<(&'a str, &'a str), u16>,
}

impl<'a> Session<'a> {
    pub fn start(
        graph: &'a Graph,
        package: &CheckedPackage,
        bindings: BTreeMap<(&'a str, &'a str), Selection<'a>>,
        language: &'a str,
    ) -> Result<Self> {
        validate(graph)?;
        ensure!(
            matches!(language, "en" | "es"),
            "unsupported session language"
        );
        let mut expected = 0;
        for node in &graph.nodes {
            if let Node::Playback { id, selections, .. } = node {
                for edition in selections {
                    expected += 1;
                    let selection = bindings
                        .get(&(id.as_str(), edition.language.as_str()))
                        .ok_or_else(|| anyhow!("missing playback binding for {id}"))?;
                    ensure!(
                        selection.edition == edition.edition
                            && selection.language == edition.language,
                        "binding differs from authored edition at {id}"
                    );
                    admit_checked_selection(package, *selection)?;
                }
            }
        }
        ensure!(bindings.len() == expected, "unexpected playback bindings");
        let mut session = Self {
            graph,
            bindings,
            language,
            node: None,
            visit: 0,
            frame: None,
            paused: false,
            focus: 0,
            playback: Playback::default(),
            repeats: BTreeMap::new(),
        };
        session.enter(&graph.entry);
        Ok(session)
    }

    fn enter(&mut self, id: &str) {
        self.node = self.graph.nodes.iter().find(|node| node.id() == id);
        self.visit += 1;
        self.paused = false;
        self.focus = 0;
        self.frame = self.selection().map(|selection| selection.first_frame);
    }

    pub fn node(&self) -> Option<&str> {
        self.node.map(Node::id)
    }

    pub fn visit(&self) -> u64 {
        self.visit
    }

    pub fn selection(&self) -> Option<Selection<'a>> {
        let node = self.node?;
        self.bindings.get(&(node.id(), self.language)).copied()
    }

    pub fn frame(&self) -> Option<u64> {
        self.frame
    }

    pub fn choices(&self) -> Vec<(&str, &str)> {
        let Some(Node::Choice { id, choices }) = self.node else {
            return Vec::new();
        };
        choices
            .iter()
            .filter(|choice| {
                choice.traversals.is_none_or(|limit| {
                    self.repeats
                        .get(&(id.as_str(), choice.id.as_str()))
                        .copied()
                        .unwrap_or(0)
                        < limit
                })
            })
            .map(|choice| (choice.id.as_str(), choice.labels[self.language].as_str()))
            .collect()
    }

    pub fn focused_choice(&self) -> Option<(&str, &str)> {
        self.choices().get(self.focus).copied()
    }

    /// Translate focus navigation into the same authored ID as text input.
    pub fn choice_key(&mut self, visit: u64, key: ChoiceKey) -> bool {
        let choices = self.choices();
        if visit != self.visit || choices.is_empty() {
            return false;
        }
        match key {
            ChoiceKey::Next => self.focus = (self.focus + 1).min(choices.len() - 1),
            ChoiceKey::Previous => self.focus = self.focus.saturating_sub(1),
            ChoiceKey::Activate => {
                let id = choices[self.focus].0.to_owned();
                return self.choose(visit, &id);
            }
        }
        true
    }

    pub fn choose(&mut self, visit: u64, choice_id: &str) -> bool {
        if visit != self.visit || !self.choices().iter().any(|(id, _)| *id == choice_id) {
            return false;
        }
        let Some(Node::Choice { id, choices }) = self.node else {
            return false;
        };
        let choice = choices
            .iter()
            .find(|choice| choice.id == choice_id)
            .expect("enabled choice");
        if choice.traversals.is_some() {
            *self.repeats.entry((id, &choice.id)).or_default() += 1;
        }
        self.enter(&choice.target);
        true
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, visit: u64, paused: bool) -> bool {
        if visit != self.visit || self.frame.is_none() {
            return false;
        }
        self.paused = paused;
        true
    }

    pub fn seek(&mut self, visit: u64, frame: u64) -> bool {
        if visit != self.visit || !self.paused {
            return false;
        }
        let Some(span) = self.selection() else {
            return false;
        };
        if !(span.first_frame..span.end_frame).contains(&frame) {
            return false;
        }
        self.frame = Some(frame);
        true
    }

    /// Feed a confirmed edition-frame observation. The visit is the playback generation.
    pub fn observe(&mut self, visit: u64, frame: u64) -> bool {
        if visit != self.visit || self.paused {
            return false;
        }
        let Some(Node::Playback { next, .. }) = self.node else {
            return false;
        };
        let span = self.selection().expect("admitted playback binding");
        if frame < self.frame.expect("playback cursor") || frame > span.end_frame {
            return false;
        }
        if frame == span.end_frame {
            self.enter(next);
        } else {
            self.frame = Some(frame);
        }
        true
    }

    pub fn restart_in(&mut self, language: &'a str) -> Result<()> {
        ensure!(
            matches!(language, "en" | "es"),
            "unsupported session language"
        );
        self.language = language;
        self.repeats.clear();
        self.enter(&self.graph.entry);
        Ok(())
    }

    /// Submit checked PCM to a local sink and advance only after it accepts the chunk.
    pub fn deliver(
        &mut self,
        package: &CheckedPackage,
        candidate: &(impl InputFiles + ?Sized),
        max_frames: usize,
        submit: impl FnMut(&[u8]) -> Result<()>,
    ) -> Result<usize> {
        let mut playback = std::mem::take(&mut self.playback);
        let result = playback.deliver(self, package, candidate, max_frames, submit);
        self.playback = playback;
        result
    }

    pub fn close(&mut self) {
        self.playback = Playback::default();
        self.node = None;
        self.frame = None;
        self.paused = false;
        self.focus = 0;
        self.repeats.clear();
    }
}
