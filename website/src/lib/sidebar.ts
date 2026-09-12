import { update } from 'fumadocs-core/source';
import type { StaticSource } from 'fumadocs-core/source';

// `docs/` carries no meta files and must not grow any: it is the repository's
// own Markdown, checked by the docs gates and read directly on GitHub. The
// sidebar is therefore generated here, from two inputs.
//
// Page order inside a folder comes from the `sidebar.order` frontmatter that
// every page already carries. Folder titles and the editorial grouping of the
// three hand-ordered sections come from the tables below, because neither is
// derivable from the content.

type PageEntry = { name: string; order: number; title: string };

// The name shown on a folder row. A directory name is not a title:
// `firebase` holds the Firestore surface and `kv` holds Redis.
const FOLDER_TITLES: Record<string, string> = {
  'get-started': 'Get started',
  developers: 'Developers',
  agents: 'Agents',
  operators: 'Operators',
  concepts: 'Concepts',
  reference: 'Reference',
  'concepts/architecture': 'Architecture',
  'developers/convex': 'Convex',
  'developers/firebase': 'Firestore',
  'developers/cloud-functions': 'Cloud Functions',
  'developers/mongodb': 'MongoDB',
  'developers/dynamodb': 'DynamoDB',
  'developers/native': 'Native API',
  'developers/runtimes/nodejs': 'Node.js runtime',
  'reference/sdk': 'SDK',
  'reference/convex': 'Convex',
  'reference/firebase': 'Firestore',
  'reference/cloud-functions': 'Cloud Functions',
  'reference/mongodb': 'MongoDB',
  'reference/dynamodb': 'DynamoDB',
  'reference/s3': 'S3',
  'reference/cloudflare': 'Cloudflare',
  'reference/kv': 'KV (Redis)',
  'reference/native': 'Native API',
  'reference/runtimes': 'Node.js runtime',
};

// The six groups, in reading order. A new top-level directory in `docs/` does
// not appear until it is named here and in `PUBLISHED_GROUPS`.
//
// The first row is a link, not a folder: `/docs/` is the documentation landing
// page and has no Markdown behind it.
const ROOT_PAGES = [
  '[Overview](/docs/)',
  'get-started',
  'developers',
  'agents',
  'operators',
  'concepts',
  'reference',
];

// Three sections interleave pages with folders under headings, so their order
// cannot come from `sidebar.order` alone. `---Name---` is a heading and
// `...folder` lifts a folder's pages to this level under it.
//
// No list names `index`. A folder's index page becomes the clickable folder
// row, so naming it here would show it twice.
const EXPLICIT_PAGES: Record<string, string[]> = {
  developers: [
    'first-app',
    'auth',
    '---Adapters---',
    'adapters',
    'convex',
    'firebase',
    'cloud-functions',
    'mongodb',
    'dynamodb',
    'native',
    '---Node.js runtime---',
    '...runtimes/nodejs',
  ],
  concepts: [
    'how-nimbus-works',
    'data-and-mutations',
    'tenant-isolation',
    'adapter-boundary',
    'runtime-permissions',
    'nodejs-runtime',
    'resource-model',
    'scaling',
    'architecture',
  ],
  reference: [
    'cli',
    'configuration',
    'deploy-admin-api',
    'current-capabilities',
    '---SDK---',
    '...sdk',
    '---Adapters---',
    'adapter-capabilities',
    'convex',
    'firebase',
    'cloud-functions',
    'mongodb',
    'dynamodb',
    's3',
    'cloudflare',
    'kv',
    'native',
    '---Node.js runtime---',
    '...runtimes',
  ],
};

// The six group directories, without the landing-page link that heads the
// root list.
const GROUP_DIRS = ROOT_PAGES.filter((page) => !page.startsWith('['));

function dirOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut === -1 ? '' : path.slice(0, cut);
}

function baseOf(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1).replace(/\.mdx?$/, '');
}

// The declared order, then the title. The order values are hand-maintained
// and do tie in a few folders; the title breaks the tie so the sidebar is
// stable across builds instead of following directory read order.
function byOrder(a: PageEntry, b: PageEntry): number {
  if (a.order !== b.order) return a.order - b.order;
  return a.title.localeCompare(b.title);
}

/**
 * Attach a generated meta file to every folder of a source built over
 * `docs/`, so the sidebar reads in the intended order under the intended
 * headings.
 */
export function withSidebar<S extends StaticSource>(source: S): S {
  return update(source)
    .files((files) => {
      const pagesByDir = new Map<string, PageEntry[]>();
      const dirs = new Set<string>(['']);

      for (const file of files) {
        if (file.type !== 'page') continue;
        const dir = dirOf(file.path);
        // Register the folder and every folder above it, so a directory that
        // holds only subdirectories still gets a meta.
        for (let at = dir; ; at = dirOf(at)) {
          dirs.add(at);
          if (at === '') break;
        }
        const data = file.data as {
          title?: string;
          sidebar?: { order?: number; label?: string };
        };
        const list = pagesByDir.get(dir) ?? [];
        list.push({
          name: baseOf(file.path),
          order: data.sidebar?.order ?? Number.MAX_SAFE_INTEGER,
          title: data.sidebar?.label ?? data.title ?? '',
        });
        pagesByDir.set(dir, list);
      }

      const metas = [];
      for (const dir of dirs) {
        // A directory that holds no pages of its own, only one subdirectory,
        // is a path segment and not a section. `developers/runtimes` is the
        // only one: the parent points straight at `runtimes/nodejs`.
        if (dir !== '' && !pagesByDir.has(dir)) continue;

        const explicit = dir === '' ? ROOT_PAGES : EXPLICIT_PAGES[dir];
        const children = [...(pagesByDir.get(dir) ?? [])]
          // The index page is the folder row itself. Listing it would put the
          // same page in the sidebar twice.
          .filter((page) => page.name !== 'index')
          .sort(byOrder);
        const pages = explicit ?? children.map((page) => page.name);

        metas.push({
          type: 'meta' as const,
          path: dir === '' ? 'meta.json' : `${dir}/meta.json`,
          data: {
            title: FOLDER_TITLES[dir],
            pages,
            // The six groups stand open. Their adapter subfolders do not:
            // eleven expanded adapters would bury everything else.
            defaultOpen: GROUP_DIRS.includes(dir) || undefined,
          },
        });
      }

      return [...files, ...metas];
    })
    .build() as S;
}
