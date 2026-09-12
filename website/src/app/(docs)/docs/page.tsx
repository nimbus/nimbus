import type { Metadata } from 'next';
import { Card, Cards } from 'fumadocs-ui/components/card';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
} from 'fumadocs-ui/layouts/docs/page';

export const metadata: Metadata = {
  title: 'Documentation',
  description:
    'Every path through the Nimbus documentation: quickstarts, adapter guides, agent sandboxes, operations, and the reference.',
  alternates: { canonical: '/docs/' },
};

// The landing page of the documentation. The marketing pitch is the site
// home at `/`; this page is navigation, so it holds the paths and nothing
// that repeats the pitch.
export default function Page() {
  return (
    <DocsPage>
      <DocsTitle>Documentation</DocsTitle>
      <DocsDescription>
        Nimbus is one binary that speaks Convex, Firestore, Cloud Functions,
        MongoDB, and DynamoDB. Start where your work starts.
      </DocsDescription>
      <DocsBody>
        <h2>Find your path</h2>
        <Cards>
          <Card
            href="/get-started/quickstart/"
            title="Developer quickstart"
            description="Run nimbus dev, write a function, and call it from an app."
          />
          <Card
            href="/get-started/self-host/"
            title="Self-host quickstart"
            description="Put one binary on one machine and keep it running."
          />
          <Card
            href="/agents/"
            title="Agent sandboxes"
            description="Give an agent a filesystem, a shell, and a network policy."
          />
          <Card
            href="/get-started/deploy/"
            title="Deploy to production"
            description="Promote the same project from nimbus dev to nimbus deploy."
          />
          <Card
            href="/get-started/from-convex/"
            title="Coming from Convex"
            description="What carries over, what changes, and what to check first."
          />
          <Card
            href="/concepts/"
            title="Concepts and architecture"
            description="How the engine, the adapters, and the runtime fit together."
          />
        </Cards>

        <h2>By surface</h2>
        <Cards>
          <Card
            href="/developers/"
            title="Developers"
            description="Build on Nimbus through the adapter your client already speaks."
          />
          <Card
            href="/operators/"
            title="Operators"
            description="Deploy, secure, back up, scale, and observe a Nimbus server."
          />
          <Card
            href="/reference/"
            title="Reference"
            description="The CLI, the configuration, the SDKs, and per-adapter coverage."
          />
        </Cards>
      </DocsBody>
    </DocsPage>
  );
}
