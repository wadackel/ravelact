// "Powered by ravelact vX.Y.Z" credit pill anchored to the bottom-left of
// the graph canvas. The version is inlined at Vite build time via
// `__RAVELACT_VERSION__` (see web/vite.config.ts), so the binary always
// advertises the exact version it was built with.

const REPO_URL = "https://github.com/wadackel/ravelact";

export function PoweredBy() {
  const version = __RAVELACT_VERSION__;
  const label = `Powered by ravelact v${version} — opens GitHub repository in a new tab`;
  return (
    <a
      href={REPO_URL}
      target="_blank"
      rel="noopener noreferrer"
      aria-label={label}
      className="absolute bottom-2 left-2 z-10 inline-flex items-center gap-1.5 bg-bg-elev/80 backdrop-blur-sm border border-border rounded-md px-2 py-1 text-fg-muted text-[11px] no-underline transition hover:text-fg hover:bg-bg-elev focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 16 16"
        fill="currentColor"
        aria-hidden="true"
        xmlns="http://www.w3.org/2000/svg"
      >
        <path
          fillRule="evenodd"
          clipRule="evenodd"
          d="M8 0C3.58 0 0 3.58 0 8a8 8 0 0 0 5.47 7.59c.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8Z"
        />
      </svg>
      <span>
        Powered by ravelact <span className="text-fg-dim">v{version}</span>
      </span>
      <span aria-hidden="true">↗</span>
    </a>
  );
}
