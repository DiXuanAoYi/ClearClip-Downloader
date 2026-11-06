/**
 * 三方合并（和后端 settings::merge3 一样的规则）：base 是表单打开时的那份，ours 是表单现在的，theirs 是别处改过后的最新值。
 * 表单没改的字段用最新值，表单改了的保留表单的；对象逐层合并，数组和值整体比较。
 */
export function merge3<T>(base: T, ours: T, theirs: T): T {
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b)
  if (same(ours, base)) return clone(theirs)
  if (same(theirs, base)) return ours
  const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
  if (isObj(base) && isObj(ours) && isObj(theirs)) {
    const out: Record<string, unknown> = {}
    for (const k of new Set([...Object.keys(base), ...Object.keys(ours), ...Object.keys(theirs)])) {
      const v = merge3(base[k], ours[k], theirs[k])
      if (v !== undefined) out[k] = v
    }
    return out as T
  }
  return ours
}

export const clone = <T>(v: T): T => (v === undefined ? v : (JSON.parse(JSON.stringify(v)) as T))
