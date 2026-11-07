//! Owner-admitted presentation groups. These never create or merge asset IDs.
use super::*;
use regex::{Regex, RegexBuilder};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matcher {
    #[serde(default)]
    pub definition: Option<String>,
    #[serde(default = "id_pointer")]
    pub pointer: String,
    pub pattern: String,
    pub key: String,
    #[serde(default)]
    pub title: Option<String>,
}
fn id_pointer() -> String {
    "/id".into()
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub label: String,
    pub pointer: String,
    pub pattern: String,
    #[serde(default)]
    pub selected_slot_prefix: Option<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facet {
    pub label: String,
    pub pattern: String,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub roots: Vec<Matcher>,
    #[serde(default = "top_level")]
    pub top_level: bool,
    #[serde(default)]
    pub parent: Option<Matcher>,
    #[serde(default)]
    pub members: Vec<Matcher>,
    #[serde(default)]
    pub stages: Vec<Stage>,
    #[serde(default)]
    pub facets: Vec<Facet>,
    #[serde(default)]
    pub count_state: Option<String>,
    #[serde(default)]
    pub count_label: Option<String>,
    #[serde(default)]
    pub version_pointer: Option<String>,
    #[serde(default)]
    pub version_role: Option<String>,
    #[serde(default)]
    pub version_slot_prefix: Option<String>,
    #[serde(default)]
    pub delivery_pattern: Option<String>,
}
fn top_level() -> bool {
    true
}
fn regex(pattern: &str) -> Result<Regex> {
    if pattern.len() > 512 || !pattern.starts_with('^') {
        return Err("Collection patterns must be bounded and anchored".into());
    }
    RegexBuilder::new(pattern)
        .size_limit(1024 * 1024)
        .dfa_size_limit(1024 * 1024)
        .build()
        .map_err(|_| "Invalid collection pattern".into())
}
pub fn validate(collections: &[Collection]) -> Result<()> {
    if collections.len() > 32 {
        return Err("Collection budget exceeded".into());
    }
    let mut ids = BTreeSet::new();
    for c in collections {
        if !token(&c.id)
            || !ids.insert(&c.id)
            || c.label.is_empty()
            || c.label.len() > 120
            || c.roots.is_empty()
            || c.roots.len() > 16
            || c.members.len() > 16
            || c.stages.len() > 16
            || c.facets.len() > 32
        {
            return Err("Invalid collection admission".into());
        }
        for m in c.roots.iter().chain(&c.members).chain(c.parent.iter()) {
            if !pointer(&m.pointer)
                || m.key.is_empty()
                || m.key.len() > 256
                || m.title.as_ref().is_some_and(|s| s.len() > 256)
            {
                return Err("Invalid work projection".into());
            }
            regex(&m.pattern)?;
        }
        for s in &c.stages {
            if !pointer(&s.pointer)
                || s.label.is_empty()
                || s.label.len() > 120
                || s.selected_slot_prefix
                    .as_ref()
                    .is_some_and(|s| s.len() > 120)
            {
                return Err("Invalid state projection".into());
            }
            regex(&s.pattern)?;
        }
        for f in &c.facets {
            if f.label.len() > 120 {
                return Err("Invalid facet".into());
            }
            regex(&f.pattern)?;
        }
        if c.count_state
            .as_ref()
            .is_some_and(|state| !c.stages.iter().any(|s| &s.label == state))
            || c.version_pointer.as_ref().is_some_and(|p| !pointer(p))
        {
            return Err("Unknown count state or version field".into());
        }
        if let Some(p) = &c.delivery_pattern {
            regex(p)?;
        }
    }
    Ok(())
}
fn field<'a>(a: &'a Resolved, pointer: &str) -> Option<&'a str> {
    if pointer == "/id" {
        return Some(&a.reference().id);
    }
    let (prefix, rest) = pointer.strip_prefix('/')?.split_once('/')?;
    let map = match prefix {
        "authored" => a.authored(),
        "status" => a.status(),
        _ => return None,
    };
    let (head, tail) = rest
        .split_once('/')
        .map_or((rest, None), |(a, b)| (a, Some(b)));
    let v = map.get(head)?;
    match tail {
        Some(t) => v.pointer(&format!("/{t}"))?,
        None => v,
    }
    .as_str()
}
fn matched(a: &Resolved, m: &Matcher, re: &Regex) -> Option<(String, String)> {
    if m.definition
        .as_ref()
        .is_some_and(|id| id != &a.definition().id)
    {
        return None;
    }
    let caps = re.captures(field(a, &m.pointer)?)?;
    let mut key = String::new();
    caps.expand(&m.key, &mut key);
    if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
        return None;
    }
    let mut title = String::new();
    if let Some(t) = &m.title {
        caps.expand(t, &mut title);
    } else {
        title = key.clone();
    }
    Some((key, title.chars().take(240).collect()))
}
#[derive(Clone, Serialize)]
pub struct Work {
    pub project: String,
    pub id: String,
    pub collection: String,
    pub category: String,
    pub title: String,
    pub icon: String,
    pub state: String,
    pub version: Option<String>,
    pub facets: Vec<String>,
    pub root: String,
    pub member_count: usize,
    pub top_level: bool,
    pub parent: Option<String>,
    #[serde(skip)]
    pub(super) members: BTreeSet<usize>,
    #[serde(skip)]
    priority: (bool, usize, usize, String),
}
pub fn build(collections: &[Collection], snapshot: &AssetSnapshot, cards: &[Card]) -> Vec<Work> {
    let assets: Vec<_> = snapshot.selected_records().collect();
    let mut result = Vec::new();
    for c in collections {
        let roots: Vec<_> = c
            .roots
            .iter()
            .filter_map(|m| regex(&m.pattern).ok().map(|r| (m, r)))
            .collect();
        let members: Vec<_> = c
            .members
            .iter()
            .filter_map(|m| regex(&m.pattern).ok().map(|r| (m, r)))
            .collect();
        let stages: Vec<_> = c
            .stages
            .iter()
            .filter_map(|s| regex(&s.pattern).ok().map(|r| (s, r)))
            .collect();
        let mut groups = BTreeMap::<String, Work>::new();
        for (i, a) in assets.iter().enumerate() {
            for (rank, (m, re)) in roots.iter().enumerate() {
                let Some((key, mut title)) = matched(a, m, re) else {
                    continue;
                };
                if m.title.is_none() {
                    title = cards[i].title.clone();
                } else {
                    title = title.replace('-', " ");
                }
                let id = &a.reference().id;
                let priority = (id != &key, rank, id.len(), id.clone());
                let replace = groups.get(&key).is_none_or(|w| priority < w.priority);
                if replace {
                    let state = stages
                        .iter()
                        .find(|(s, re)| {
                            field(a, &s.pointer).is_some_and(|v| {
                                re.is_match(v)
                                    && a.selected().is_ok()
                                    && s.selected_slot_prefix.as_ref().is_none_or(|p| {
                                        a.stream(&format!("{p}{v}")).ok() == a.selected().ok()
                                    })
                            })
                        })
                        .map_or(
                            if a.selected().is_ok() {
                                "Registered"
                            } else {
                                "Planned"
                            },
                            |(s, _)| s.label.as_str(),
                        )
                        .to_owned();
                    let previous = groups
                        .remove(&key)
                        .map_or_else(BTreeSet::new, |w| w.members);
                    let facets = c
                        .facets
                        .iter()
                        .filter(|f| regex(&f.pattern).is_ok_and(|r| r.is_match(&key)))
                        .map(|f| f.label.clone())
                        .collect();
                    groups.insert(
                        key.clone(),
                        Work {
                            project: cards[i].project.clone(),
                            id: key.clone(),
                            collection: c.id.clone(),
                            category: c.label.clone(),
                            title,
                            icon: c.icon.clone(),
                            state,
                            version: c
                                .version_pointer
                                .as_ref()
                                .and_then(|p| field(a, p))
                                .map(str::to_owned),
                            facets,
                            root: id.clone(),
                            member_count: 0,
                            top_level: c.top_level,
                            parent: c.parent.as_ref().and_then(|m| {
                                regex(&m.pattern)
                                    .ok()
                                    .and_then(|r| matched(a, m, &r).map(|(key, _)| key))
                            }),
                            members: previous,
                            priority,
                        },
                    );
                }
                groups.get_mut(&key).unwrap().members.insert(i);
                break;
            }
        }
        for (i, a) in assets.iter().enumerate() {
            for (m, re) in &members {
                if let Some((key, _)) = matched(a, m, re)
                    && let Some(w) = groups.get_mut(&key)
                {
                    w.members.insert(i);
                }
            }
        }
        for w in groups.values_mut() {
            w.member_count = w.members.len();
        }
        result.extend(groups.into_values());
    }
    result
}
pub fn summaries(collections: &[Collection], works: &[Work]) -> Value {
    json!(collections.iter().map(|c| {
        let items:Vec<_>=works.iter().filter(|w|w.collection==c.id).collect();
        let mut states=BTreeMap::<&str,usize>::new();let mut facets=BTreeMap::<&str,usize>::new();let mut state_facets=BTreeMap::<&str,BTreeMap<&str,usize>>::new();
        for w in &items {*states.entry(&w.state).or_default()+=1;for f in &w.facets {*facets.entry(f).or_default()+=1;*state_facets.entry(&w.state).or_default().entry(f).or_default()+=1;}}
        let count=items.iter().filter(|w|c.count_state.as_ref().is_none_or(|s|s==&w.state)).count();
        json!({"id":c.id,"label":c.label,"top_level":c.top_level,"count":items.len(),"states":states,"facets":facets,"state_facets":state_facets,"count_state":c.count_state,"count_label":c.count_label.as_ref().unwrap_or(&c.label),"count_value":count})
    }).collect::<Vec<_>>())
}
impl Catalogue {
    pub fn work_list(
        &self,
        scope: (&str, &str),
        state: &str,
        facet: &str,
        search: &str,
        offset: usize,
    ) -> Result<Value> {
        let (project, collection) = scope;
        if offset > 1_000_000 || search.len() > 256 || facet.len() > 120 || state.len() > 120 {
            return Err("Work-list budget exceeded".into());
        }
        let p = self.projects.get(project).ok_or("Choose one project")?;
        let q = search.to_lowercase();
        let rows: Vec<_> = p
            .works
            .iter()
            .filter(|w| {
                (if collection.is_empty() {
                    w.top_level
                } else {
                    w.collection == collection
                }) && (state.is_empty() || w.state == state)
                    && (facet.is_empty() || w.facets.iter().any(|f| f == facet))
                    && (q.is_empty() || format!("{} {}", w.title, w.id).to_lowercase().contains(&q))
            })
            .collect();
        Ok(
            json!({"items":rows.iter().skip(offset).take(100).collect::<Vec<_>>(),"total":rows.len(),"snapshots":{project:&p.commit}}),
        )
    }
    pub fn work_detail(
        &self,
        scope: (&str, &str),
        id: &str,
        commit: &str,
        category: &str,
        offset: usize,
    ) -> Result<Value> {
        let (project, collection) = scope;
        let p = self.projects.get(project).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        if offset > 1_000_000 || category.len() > 256 {
            return Err("Member-list budget exceeded".into());
        }
        let w = p
            .works
            .iter()
            .find(|w| w.collection == collection && w.id == id)
            .ok_or("Unknown work")?;
        let c = p
            .enrollment
            .collections
            .iter()
            .find(|c| c.id == collection)
            .ok_or("Unknown collection")?;
        let root = p
            .snapshot
            .resolve_id(&w.root)
            .map_err(|_| "Root unavailable")?;
        let versions:Vec<_>=root.stream_bindings().iter().filter(|s|c.version_role.as_ref().is_some_and(|r|r==&s.role)).map(|s|json!({"label":s.slot,"current":root.selected().ok()==Some(&s.object)&&c.version_slot_prefix.as_ref().zip(w.version.as_ref()).is_some_and(|(p,v)|s.slot==format!("{p}{v}")),"object":s.object})).collect();
        let mut categories = BTreeMap::<&str, usize>::new();
        let mut delivery = Vec::new();
        let delivery_re = c.delivery_pattern.as_ref().and_then(|p| regex(p).ok());
        for i in &w.members {
            let card = &p.cards[*i];
            *categories.entry(&card.category).or_default() += 1;
            if delivery_re.as_ref().is_some_and(|r| r.is_match(&card.id)) {
                delivery.push(card);
            }
        }
        let all: Vec<_> = w
            .members
            .iter()
            .map(|i| &p.cards[*i])
            .filter(|c| category.is_empty() || c.category == category)
            .collect();
        let children: Vec<_> = p
            .works
            .iter()
            .filter(|child| child.parent.as_ref() == Some(&w.id) && child.id != w.id)
            .collect();
        let mut parents = Vec::new();
        let mut seen = BTreeSet::from([w.id.as_str()]);
        let mut parent = w.parent.as_ref();
        while let Some(id) = parent {
            if parents.len() >= 16 || !seen.insert(id.as_str()) {
                break;
            }
            let Some(found) = p.works.iter().find(|w| &w.id == id) else {
                break;
            };
            parents.push(found);
            parent = found.parent.as_ref();
        }
        Ok(
            json!({"work":w,"commit":p.commit,"children":children.iter().take(100).collect::<Vec<_>>(),"children_total":children.len(),"parents":parents,"versions":versions,"deliveries":delivery.into_iter().take(100).collect::<Vec<_>>(),"member_categories":categories,"members":all.iter().skip(offset).take(100).collect::<Vec<_>>(),"member_total":all.len(),"offset":offset,"root":p.cards.iter().find(|c|c.id==w.root),"remote_content_verified":false,"authorizes_eviction":false}),
        )
    }

    pub fn search(&self, project: &str, kind: &str, q: &str, offset: usize) -> Result<Value> {
        if q.trim().is_empty() || q.len() > 256 || kind.len() > 80 || offset > 1_000_000 {
            return Err("Search budget exceeded".into());
        }
        if project != "all" && !self.projects.contains_key(project) {
            return Err("Unknown project".into());
        }
        let query = q.to_lowercase();
        let mut rows = Vec::new();
        let mut snapshots = BTreeMap::new();
        for (id, p) in self
            .projects
            .iter()
            .filter(|(id, _)| project == "all" || id.as_str() == project)
        {
            snapshots.insert(id, &p.commit);
            for w in &p.works {
                if !kind.is_empty() && w.collection != kind {
                    continue;
                }
                let direct = format!("{} {} {}", w.id, w.title, w.category)
                    .to_lowercase()
                    .contains(&query);
                let member = w.members.iter().any(|i| {
                    let c = &p.cards[*i];
                    format!(
                        "{} {} {} {}",
                        c.id,
                        c.title,
                        c.definition,
                        c.selected.as_ref().map_or("", |o| o.sha256.as_str())
                    )
                    .to_lowercase()
                    .contains(&query)
                });
                if direct || member {
                    rows.push((!direct, w));
                }
            }
        }
        rows.sort_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.project.cmp(&b.1.project))
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        Ok(
            json!({"total":rows.len(),"items":rows.iter().skip(offset).take(100).map(|(_,w)|w).collect::<Vec<_>>(),"snapshots":snapshots,"coverage":"Registered catalog IDs, labels, types and selected hashes; package/media text is not indexed."}),
        )
    }
    pub fn children(
        &self,
        scope: (&str, &str),
        id: &str,
        commit: &str,
        kind: &str,
        offset: usize,
    ) -> Result<Value> {
        if kind.len() > 80 || offset > 1_000_000 {
            return Err("Child-list budget exceeded".into());
        }
        let p = self.projects.get(scope.0).ok_or("Unknown project")?;
        p.check_snapshot(commit)?;
        if !p
            .works
            .iter()
            .any(|w| w.collection == scope.1 && w.id == id)
        {
            return Err("Unknown work".into());
        }
        let children: Vec<_> = p
            .works
            .iter()
            .filter(|w| w.parent.as_deref() == Some(id) && w.id != id)
            .collect();
        let mut kinds = BTreeMap::<&str, Value>::new();
        for w in &children {
            let n = kinds
                .entry(&w.collection)
                .or_insert_with(|| json!({"label":w.category,"count":0}));
            n["count"] = json!(n["count"].as_u64().unwrap_or(0) + 1);
        }
        let rows: Vec<_> = children
            .iter()
            .filter(|w| kind.is_empty() || w.collection == kind)
            .collect();
        Ok(
            json!({"items":rows.iter().skip(offset).take(100).collect::<Vec<_>>(),"total":rows.len(),"kinds":kinds,"commit":p.commit}),
        )
    }
}
