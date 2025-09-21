
//#region Imports
import path from 'path';
//#endregion

//#region Set ENV Variables

// Sets the CONFIG directory
process.env.NODE_CONFIG_DIR = path.join(__dirname, '..', 'config');

//#endregion

// Imports the server as to run the actual program
require('./Server');