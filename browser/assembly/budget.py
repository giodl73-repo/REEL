"""Separate initial browser runtime from optional complete source/legal delivery."""
RUNTIME_BUDGET_BYTES=524_288
SITE_BUDGET_BYTES=8_388_608
RUNTIME_FILES=('index.html','style.css','app.js','worker.js','graph.json','pkg/reel_assembly_workbench.js','pkg/reel_assembly_workbench_bg.wasm')

def runtime_bytes(site):
 return sum((site/name).stat().st_size for name in RUNTIME_FILES)+(site.parent/'index.html').stat().st_size
