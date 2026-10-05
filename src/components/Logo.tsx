/**
 * Kikitori's logo: the name in round strokes with the red bird of the app icon as its o, the same
 * drawing as `docs/images/kikitori-logo.svg`, in the theme's ink, red and background.
 */
export function Logo({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="-12 -186 793 186" className={className} role="img" aria-label="Kikitori">
      <g className="stroke-fg" fill="none" strokeWidth={24} strokeLinecap="round" strokeLinejoin="round">
        <path d="M0 -174L0 -12M0 -50L60 -108M21.6 -70.88L64 -12" />
        <path d="M124 -108L124 -12" />
        <path d="M194 -174L194 -12M194 -50L254 -108M215.6 -70.88L258 -12" />
        <path d="M318 -108L318 -12" />
        <path d="M396 -146L396 -42A30 30 0 0 0 426 -12L432 -12M368 -108L438 -108" />
        <path d="M677.47 -108L677.47 -12M677.47 -48C677.47 -90 693.47 -108 723.47 -108" />
        <path d="M765.47 -108L765.47 -12" />
      </g>
      <g className="fill-fg">
        <circle cx="124" cy="-160.88" r="14.88" />
        <circle cx="318" cy="-160.88" r="14.88" />
        <circle cx="765.47" cy="-160.88" r="14.88" />
      </g>
      <path d="M476 -93.9L547.16 -109.71A54 54 0 1 1 511.86 -30.43Z" className="fill-rec" />
      <path d="M606.94 -56.18L637.47 -76.6L600.93 -80.29Z" className="fill-fg stroke-fg" strokeWidth={2.54} strokeLinejoin="round" />
      <circle cx="581.56" cy="-75.36" r="5.4" className="fill-bg" />
    </svg>
  );
}
