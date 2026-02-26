// INTRO version: Legacy notes module with placeholder tools that COLLIDE

export const noteTools = [
  { name: 'create_note', description: 'Create note' },
  { name: 'list_notes', description: 'List notes' },
  // BUG: These collide with new attachments!
  { name: 'list_attachments', description: 'Legacy: List attachments' },
  { name: 'attach_file', description: 'Legacy: Attach file' },
  { name: 'delete_attachment', description: 'Legacy: Delete attachment' }
];
