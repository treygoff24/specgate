import got from "got";

export async function fetchUser(id: string): Promise<any> {
  const response = await got(`https://api.example.com/users/${id}`).json();
  return response;
}

export async function fetchPosts(): Promise<any[]> {
  const response = await got("https://api.example.com/posts").json();
  return response as any[];
}