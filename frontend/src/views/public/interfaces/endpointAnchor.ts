/**
 * Shared anchor-id convention for endpoint cards and the overview tables that
 * link to them. Content cards use absolute base URLs and sometimes combined
 * method labels ('GET / POST'), so both are normalized here to keep ids stable
 * and computable from the short method/path pairs in endpointRows.ts.
 */

/** First HTTP verb of a (possibly combined) method label, e.g. 'GET / POST' -> 'GET'. */
function firstMethod(method: string): string {
  return method.split('/')[0].trim()
}

/** Path portion of an endpoint path that may be given as a full URL. */
function pathOnly(path: string): string {
  return path.startsWith('http') ? new URL(path).pathname : path
}

/** Stable anchor id per endpoint card (e.g. 'post-v1-videos'). */
export function endpointAnchorFor(item: { method: string; path: string }): string {
  return `${firstMethod(item.method)}-${pathOnly(item.path)}`
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}
