// FIX version: Registry works with unique tools

import { attachmentTools } from './attachments-fix';
import { noteTools } from './notes-fix';

const TOOL_DEFINITIONS = [...attachmentTools, ...noteTools];

// No duplicates - this works!
const toolMap = new Map();
for (const tool of TOOL_DEFINITIONS) {
  if (toolMap.has(tool.name)) {
    throw new Error(`Duplicate tool: ${tool.name}`);
  }
  toolMap.set(tool.name, tool);
}

export { TOOL_DEFINITIONS };
