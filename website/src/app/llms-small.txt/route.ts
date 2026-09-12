import { llmsSmall } from '@/lib/llms';

export const revalidate = false;

export async function GET(): Promise<Response> {
  return new Response(await llmsSmall(), {
    headers: { 'Content-Type': 'text/plain; charset=utf-8' },
  });
}
