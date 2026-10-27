"use client";

import { useEffect, useState } from "react";

/**
 * A terminal that cycles through a few highlighted `grit` sessions on the homepage. Every
 * session is rendered into the same grid cell, so the frame keeps the tallest one's height and
 * nothing jumps as it cycles. Hovering or focusing it pauses the cycle; with reduced motion it
 * stays put until a tab is picked.
 */

const INTERVAL_MS = 4500;

export function CliDemo({ examples }: { examples: { label: string; html: string }[] }) {
  const [active, setActive] = useState(0);
  const [paused, setPaused] = useState(false);

  useEffect(() => {
    if (paused || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const id = window.setInterval(() => setActive((i) => (i + 1) % examples.length), INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [paused, examples.length]);

  return (
    <div
      className="cli-demo"
      onMouseEnter={() => setPaused(true)}
      onMouseLeave={() => setPaused(false)}
      onFocus={() => setPaused(true)}
      onBlur={() => setPaused(false)}
    >
      <div className="cli-demo-tabs" role="tablist" aria-label="grit examples">
        {examples.map((ex, i) => (
          <button
            key={ex.label}
            type="button"
            role="tab"
            id={`cli-demo-tab-${i}`}
            aria-selected={i === active}
            aria-controls={`cli-demo-panel-${i}`}
            onClick={() => setActive(i)}
          >
            {ex.label}
          </button>
        ))}
      </div>
      <div className="cli-demo-panels">
        {examples.map((ex, i) => (
          <div
            key={ex.label}
            role="tabpanel"
            id={`cli-demo-panel-${i}`}
            aria-labelledby={`cli-demo-tab-${i}`}
            aria-hidden={i !== active}
            className={i === active ? "active" : undefined}
            dangerouslySetInnerHTML={{ __html: ex.html }}
          />
        ))}
      </div>
    </div>
  );
}
