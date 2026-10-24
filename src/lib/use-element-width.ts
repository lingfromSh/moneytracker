import * as React from "react";

/**
 * Width of an element in CSS pixels, kept in sync with the element and the window.
 *
 * `react-grid-layout`'s own `useContainerWidth` measures the element it renders
 * into, which is also the element the grid can inflate: after the window is
 * un-maximised the stale, wider measurement sticks and the grid keeps overflowing
 * its container. Measuring a wrapper that is allowed to shrink (see the
 * `w-full min-w-0` + `overflow-x-hidden` pair in the dashboard) keeps the value
 * honest, and the extra `resize` listener covers cases where the observer never
 * fires (for example when only the visual viewport changes).
 */
export function useElementWidth(fallback = 1024) {
  const ref = React.useRef<HTMLDivElement | null>(null);
  const [width, setWidth] = React.useState(fallback);

  const measure = React.useCallback(() => {
    const node = ref.current;
    if (!node) return;
    const next = Math.round(node.getBoundingClientRect().width);
    if (next > 0) setWidth((previous) => (previous === next ? previous : next));
  }, []);

  React.useLayoutEffect(() => {
    measure();
    const node = ref.current;
    const observer =
      typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => measure()) : null;
    if (node) observer?.observe(node);
    window.addEventListener("resize", measure);
    // The sidebar and the header settle a frame after mount.
    const frame = window.requestAnimationFrame(measure);
    const timer = window.setTimeout(measure, 120);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
      window.cancelAnimationFrame(frame);
      window.clearTimeout(timer);
    };
  }, [measure]);

  return { ref, width, measure };
}
