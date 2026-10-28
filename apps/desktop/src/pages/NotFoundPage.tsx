import { PageHeader } from '@/components/PageHeader';

export function NotFoundPage() {
  return (
    <div className="mx-(--body-gutter) pt-11 pb-16">
      <PageHeader title="Page not found" />
      <p className="type-body text-secondary">The page you were after doesn't exist here.</p>
    </div>
  );
}
