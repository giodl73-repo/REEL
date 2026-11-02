import {defineConfig} from '@playwright/test';
const live=process.env.REEL_BASE_URL;
const site=process.env.REEL_SITE_DIR || '../../target/reel-release';
export default defineConfig({testDir:'./tests',workers:1,retries:0,timeout:30000,
 use:{baseURL:live || 'http://127.0.0.1:4195/REEL/assembly/'},
 reporter:[['list'],['json',{outputFile:live?'.cache/live-browser-results.json':'.cache/browser-results.json'}]],
 webServer:live?undefined:{command:`python -m http.server 4195 --directory "${site}"`,url:'http://127.0.0.1:4195/REEL/assembly/',reuseExistingServer:false}
});
