export async function loadSecret() {
  const mod = await import('../secrets/keys');
  return mod.API_KEY;
}
