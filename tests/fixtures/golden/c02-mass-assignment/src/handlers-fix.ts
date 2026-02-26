// FIX version: Hardened - explicit field allowlist

const TASK_PATCH_FIELDS = ['title', 'updated_at'] as const;
const TASK_CREATE_FIELDS = ['title', 'user_id'] as const;

function filterFields<T extends string>(obj: any, allowed: readonly T[]): Pick<any, T> {
  const result: any = {};
  for (const key of allowed) {
    if (key in obj) {
      result[key] = obj[key];
    }
  }
  return result;
}

export async function patchTask(req: Request, db: any): Promise<Response> {
  const body = await req.json();
  const id = req.params.id;
  
  // SECURE: Explicit field allowlist
  const filtered = filterFields(body, TASK_PATCH_FIELDS);
  const result = await db.from('tasks').update({ 
    ...filtered,
    updated_at: new Date() 
  }).eq('id', id);
  
  return new Response(JSON.stringify(result));
}

export async function postTask(req: Request, db: any): Promise<Response> {
  const body = await req.json();
  
  // SECURE: Explicit field allowlist for create
  const filtered = filterFields(body, TASK_CREATE_FIELDS);
  const result = await db.from('tasks').insert({
    ...filtered,
    created_at: new Date()
  });
  
  return new Response(JSON.stringify(result));
}
