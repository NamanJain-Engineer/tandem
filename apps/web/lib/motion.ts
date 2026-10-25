/** The scroll-reveal step for the i-th of a list's items (D-1874): items
 * that enter together arrive one after another, five steps at most (DESIGN
 * §4.7), then the cycle starts again. */
export function stagger(i: number): string {
  const step = i % 5;
  return step === 0 ? "" : `stagger-${step}`;
}
