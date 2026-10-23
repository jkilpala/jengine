// Bootstrap script — loads the WASM module and initializes jEngine.
import init from '../pkg/jengine_web.js';

async function run() {
    try {
        await init();
        console.log('jEngine WASM module loaded successfully');
    } catch (e) {
        console.error('Failed to initialize jEngine:', e);
    }
}

run();
