"""Retain production serialization/hash dependency identities in the WASM closure."""
import json,pathlib,subprocess,tomllib
ROOT=pathlib.Path(__file__).resolve().parents[2]
original=tomllib.loads((ROOT/'Cargo.lock').read_text(encoding='utf-8'))['package']
adapter=tomllib.loads((ROOT/'browser/assembly/Cargo.lock').read_text(encoding='utf-8'))['package']
identity=lambda p:(p['name'],p['version'],p.get('source'))
by_identity={identity(p):p for p in original};names={p['name'] for p in original}
shared=[p for p in adapter if p['name'] in names]
for p in shared:
 assert identity(p) in by_identity,p['name']
 assert p.get('checksum')==by_identity[identity(p)].get('checksum'),p['name']
metadata=json.loads(subprocess.check_output(['cargo','+1.95.0','metadata','--manifest-path','browser/assembly/Cargo.toml','--locked','--format-version','1'],cwd=ROOT))
first_party=[p for p in metadata['packages'] if p['name'] in ['reel-assembly','reel-assembly-workbench']]
assert len(first_party)==2
for p in first_party:assert p['source'] is None and pathlib.Path(p['manifest_path']).resolve().is_relative_to(ROOT.resolve()),p['name']
print(json.dumps({'shared_production_packages':len(shared),'version_source_checksum_match':True,'first_party_manifests_contained':True}))
