import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';

import { Lockup } from '@/components/lockup';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: <Lockup />,
      url: '/',
    },
    githubUrl: 'https://github.com/nimbus/nimbus',
  };
}
