import { bookmarkApi } from './client';

export async function listUsers(
  _query?: Record<string, string>,
): Promise<{ items: any[] }> {
  return bookmarkApi.get('/users?noPaging=true');
}

export async function listRoles(): Promise<{ items: any[] }> {
  return bookmarkApi.get('/roles?noPaging=true');
}
