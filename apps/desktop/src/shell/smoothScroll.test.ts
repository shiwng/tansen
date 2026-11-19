// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { lenisOptions } from './smoothScroll';

// Pins the Lenis configuration surface: wrapper-scoped events, managed loop,
// nested-scroller passthrough, reduced-motion respect, snappy-but-present lerp.
describe('lenisOptions', () => {
  it('binds Lenis to the page scroller and keeps everything else native', () => {
    const el = document.createElement('main');
    const inner = document.createElement('div');
    el.appendChild(inner);
    const options = lenisOptions(el);
    expect(options.wrapper).toBe(el);
    expect(options.eventsTarget).toBe(el);
    expect(options.content).toBe(inner);
    expect(options.autoRaf).toBe(false);
    expect(options.allowNestedScroll).toBe(true);
    expect(options.respectReducedMotion).toBe(true);
    expect(options.wheelMultiplier).toBe(1);
    expect(options.lerp).toBe(0.12);
  });
});
