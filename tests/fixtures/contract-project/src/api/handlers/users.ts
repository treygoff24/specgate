// User handler module - exports functions matching the create-user contract

export interface CreateUserRequest {
  name: string;
  email: string;
}

export async function createUser(request: CreateUserRequest): Promise<{ id: string }> {
  // Implementation here
  return { id: "user-123" };
}
