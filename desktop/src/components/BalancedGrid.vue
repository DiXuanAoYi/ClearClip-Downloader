<script lang="ts">
import { Comment, Fragment, defineComponent, h, nextTick, onBeforeUnmount, onMounted, onUpdated, ref, watch, type PropType, type VNode } from 'vue'
import { balanceColumns, type LayoutMode } from '../utils/balance'
import { layoutMode } from '../utils/settingsLayout'

/**
 * 设置页的卡片排布：把卡片分到几列，让各列高度尽量接近，没有一行里矮卡片下面空着的位置。
 * - balance：量出每张卡片的高度，自动分栏，使最高的一列尽量矮（每列里仍按原来的先后顺序）；
 * - order：严格按原顺序，每一张放进当前最矮的一列；
 * - single：单列。
 * 列数按容器宽度定（每列至少 minCol 像素）。带 `wide` 类的卡片独占一整行，把前后分成两段各自排布。
 * 卡片的高度变了（展开、收起、加载出内容）会重新分配；正在输入时先不动，等离开输入框再排。
 */
export default defineComponent({
  name: 'BalancedGrid',
  props: {
    /** 不传就用“设置 → 通用 → 设置页排布”里选的；传了就以传的为准 */
    mode: { type: String as PropType<LayoutMode>, default: undefined },
    minCol: { type: Number, default: 340 },
    /** 最多几列：窗口再宽也不会把卡片挤成很窄的长条 */
    maxCols: { type: Number, default: 4 },
    gap: { type: Number, default: 12 },
  },
  setup(props, { slots }) {
    const root = ref<HTMLElement | null>(null)
    const curMode = (): LayoutMode => props.mode ?? layoutMode.value
    const cols = ref(1)
    const assign = ref<number[]>([])
    const heights = ref<number[]>([])
    let ro: ResizeObserver | null = null
    let pending: number[] | null = null
    let raf = 0
    let lastMode = ''

    function flatten(nodes: VNode[] | undefined, out: VNode[] = []): VNode[] {
      for (const n of nodes ?? []) {
        if (n.type === Comment) continue
        if (n.type === Fragment && Array.isArray(n.children)) flatten(n.children as VNode[], out)
        else out.push(n)
      }
      return out
    }
    const isWide = (v: VNode) => /\bwide\b/.test(String((v.props as Record<string, unknown> | null)?.class ?? ''))

    /** 每一段（相邻的非 wide 卡片）的下标；wide 卡片自成一段 */
    function segments(items: VNode[]): { wide: boolean; idx: number[] }[] {
      const out: { wide: boolean; idx: number[] }[] = []
      items.forEach((v, i) => {
        const w = isWide(v)
        const last = out[out.length - 1]
        if (!w && last && !last.wide) last.idx.push(i)
        else out.push({ wide: w, idx: [i] })
      })
      return out
    }
    function compute(items: VNode[], hs: number[]): number[] {
      const res: number[] = items.map(() => 0)
      for (const seg of segments(items)) {
        if (seg.wide) continue
        const a = balanceColumns(
          seg.idx.map((i) => hs[i] ?? 0),
          cols.value,
          curMode(),
        )
        seg.idx.forEach((i, k) => (res[i] = a[k]))
      }
      return res
    }

    function typing() {
      const a = document.activeElement
      if (!root.value || !a || !root.value.contains(a)) return false
      // 只有文本类输入框才算“正在输入”；单选、开关、按钮不算
      if (a.tagName === 'TEXTAREA') return true
      return a.tagName === 'INPUT' && !/^(radio|checkbox|button|submit|range)$/.test((a as HTMLInputElement).type)
    }
    function measure() {
      raf = 0
      const el = root.value
      if (!el) return
      const w = el.clientWidth
      const n = curMode() === 'single' ? 1 : Math.min(props.maxCols, Math.max(1, Math.floor((w + props.gap) / (props.minCol + props.gap))))
      const nodes = [...el.querySelectorAll<HTMLElement>('[data-bgi]')]
      const hs: number[] = []
      for (const e of nodes) hs[Number(e.dataset.bgi)] = e.offsetHeight
      const sameHeights = hs.length === heights.value.length && hs.every((x, i) => Math.abs(x - heights.value[i]) < 2)
      if (n === cols.value && sameHeights && assign.value.length === hs.length && lastMode === curMode()) return
      lastMode = curMode()
      heights.value = hs
      cols.value = n
      const items = flatten(slots.default?.())
      const next = compute(items, hs)
      if (typing()) {
        pending = next
        return
      }
      if (next.length !== assign.value.length || next.some((x, i) => x !== assign.value[i])) assign.value = next
    }
    function schedule() {
      if (!raf) raf = requestAnimationFrame(measure)
    }
    function observe() {
      if (!ro || !root.value) return
      ro.disconnect()
      ro.observe(root.value)
      root.value.querySelectorAll('[data-bgi]').forEach((e) => ro!.observe(e))
    }
    function onFocusOut() {
      setTimeout(() => {
        if (pending && !typing()) {
          assign.value = pending
          pending = null
        }
      }, 0)
    }

    watch(curMode, schedule)

    onMounted(() => {
      ro = new ResizeObserver(schedule)
      observe()
      root.value?.addEventListener('focusout', onFocusOut)
      void nextTick(schedule)
    })
    onUpdated(() => {
      observe()
      schedule()
    })
    onBeforeUnmount(() => {
      ro?.disconnect()
      root.value?.removeEventListener('focusout', onFocusOut)
      if (raf) cancelAnimationFrame(raf)
    })

    return () => {
      const items = flatten(slots.default?.())
      // 卡片数量变了（有的卡片按条件出现 / 消失）或还没量过：先都放第一列，渲染后量高度再分
      const a = assign.value.length === items.length ? assign.value : items.map(() => 0)
      // 每张卡片外包一层，用这一层量高度：卡片本身是组件时（多根节点、带 v-if）也量得到
      const node = (v: VNode, i: number) => h('div', { class: 'bg-item', 'data-bgi': i, key: v.key ?? `bg-${i}`, style: { minWidth: '0' } }, [v])
      const gap = `${props.gap}px`
      return h(
        'div',
        { ref: root, class: 'bgrid', style: { display: 'flex', flexDirection: 'column', gap } },
        segments(items).map((seg) =>
          seg.wide
            ? node(items[seg.idx[0]], seg.idx[0])
            : h(
                'div',
                { class: 'bg-seg', style: { display: 'flex', gap, alignItems: 'flex-start' } },
                Array.from({ length: cols.value }, (_, c) =>
                  h(
                    'div',
                    { class: 'bg-col', style: { flex: '1 1 0', minWidth: '0', display: 'flex', flexDirection: 'column', gap } },
                    seg.idx.filter((i) => (a[i] ?? 0) === c).map((i) => node(items[i], i)),
                  ),
                ),
              ),
        ),
      )
    }
  },
})
</script>
