// INTRO version: Registry that throws on duplicates

import { attachmentTools } from './attachments-intro';
import { noteTools } from './notes-intro';

const TOOL_DEFINITIONS = [...attachmentTools, ...noteTools];

// This will throw at startup!
const toolMap = new Map();
for (const tool of TOOL_DEFINITIONS) {
  if (toolMap.has(tool.name)) {
    throw new Error(`Duplicate tool: ${tool.name}`);
  }
  toolMap.set(tool.name, tool);
}

export { TOOL_DEFINITIONS };
