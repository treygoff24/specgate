import axios from "axios";

export async function fetchUser(id: string): Promise<any> {
  const response = await axios.get(`https://api.example.com/users/${id}`);
  return response.data;
}

export async function fetchPosts(): Promise<any[]> {
  const response = await axios.get("https://api.example.com/posts");
  return response.data;
}