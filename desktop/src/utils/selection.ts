/** 划词工具的小逻辑：朗读语言、是不是一个词。和浏览器扩展里的 selbar-logic.js 一致。 */
export function speechLang(text: string): string {
  const n = (re: RegExp) => (text.match(re) || []).length
  const kana = n(/[\u3040-\u30ff]/g)
  const han = n(/[\u4e00-\u9fff]/g)
  const hangul = n(/[\uac00-\ud7af]/g)
  const cyr = n(/[\u0400-\u04ff]/g)
  const latin = n(/[A-Za-z\u00c0-\u00ff]/g)
  if (kana > 0) return 'ja-JP'
  if (hangul > han && hangul > latin) return 'ko-KR'
  if (han > 0 && han >= latin / 2) return 'zh-CN'
  if (cyr > latin) return 'ru-RU'
  return 'en-US'
}

export function isWord(text: string): boolean {
  const t = text.trim()
  if (!t || t.length > 40) return false
  if (/[。！？!?；;\n]/.test(t)) return false
  if (/[\u3400-\u9fff]/.test(t)) return t.length <= 8
  return t.split(/\s+/).length <= 3
}
