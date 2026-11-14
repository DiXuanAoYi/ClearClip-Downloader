// jsdom 缺的浏览器接口：matchMedia（app store 用）、ResizeObserver（CcField 用）。
// 用普通函数而不是 vi.fn：配置里开了 restoreMocks，vi.fn 的实现会在每个用例前被清掉。
if (!window.matchMedia) {
  window.matchMedia = (q: string) =>
    ({
      matches: false,
      media: q,
      addEventListener() {},
      removeEventListener() {},
      addListener() {},
      removeListener() {},
      dispatchEvent: () => false,
      onchange: null,
    }) as unknown as MediaQueryList
}
if (!('ResizeObserver' in globalThis)) {
  ;(globalThis as unknown as Record<string, unknown>).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
}
