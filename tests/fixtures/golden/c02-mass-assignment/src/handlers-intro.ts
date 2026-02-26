// INTRO version: Vulnerable - raw body spread into mutation

export async function patchTask(req: Request, db: any): Promise<Response> {
  const body = await req.json();
  const id = req.params.id;
  
  // VULNERABLE: Direct spread of body into update
  const result = await db.from('tasks').update({ 
    ...body,  // <- BUG: allows user_id, archived_at, etc.
    updated_at: new Date() 
  }).eq('id', id);
  
  return new Response(JSON.stringify(result));
}

export async function postTask(req: Request, db: any): Promise<Response> {
  const body = await req.json();
  
  // VULNERABLE: Object.assign with raw body
  const existing = await db.from('tasks').select().single();
  const merged = Object.assign({}, existing, body); // <- BUG
  const result = await db.from('tasks').insert(merged);
  
  return new Response(JSON.stringify(result));
}
