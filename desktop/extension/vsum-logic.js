// 视频摘要的纯逻辑：字幕解析、整理成句、章节、导图布局、导出、缓存。不碰网页和浏览器接口，可以单元测试。
// 内容脚本里挂在 globalThis.ccVsum 上。
;(() => {
  if (globalThis.ccVsum) return

  // 各部分（vsum-logic-*.js）按顺序执行，把函数放进 C；这里只挑出对外的那些
  const C = {}
  for (const m of ['parse', 'page', 'maps', 'notes', 'comments']) globalThis.ccParts.vsumLogic[m](C)
  const {
    FLOW_KINDS, REWRITES, STYLES, SUGGESTIONS, addNote, baseKey, cacheKey, cardsToCsv, chapterAt, citations,
    cleanComments, cleanCueText, cmKey, commentLink, commentTime, commentsToMarkdown, contextAround, dockFrac,
    dockNext, dockTabTop, fileName, findComment, flowSvg, fmtClock, fromTextTrackCues, hashText, layoutFlow,
    layoutMindmap, lineAt, lruTrim, makeNote, mindmapSvg, normalizeFlow, normalizeLines, notesToMarkdown,
    outlineToTree, parseBiliComments, parseBilibili, parseClock, parseCount, parseDescriptionChapters,
    parseJson3, parsePoint, parseSubtitleText, parseTimedTextXml, parseYouTubePage, parseYtComments, partOf,
    pickTrack, pruneCache, resegmentLines, scriptOf, searchLines, searchNotes, sliceJson, sortNotes, splitLong,
    splitMarks, splitSummary, studyDoc, timeLink, toMarkdown, toSrt, toTxt, trimEdits, trimNotes, videoIdOf,
    videoKey, viewLine, wrapText
  } = C

  globalThis.ccVsum = {
    dockTabTop, dockFrac, dockNext,
    fmtClock, parseClock, parseJson3, parseBilibili, parseSubtitleText, cleanCueText, fromTextTrackCues, normalizeLines, parseDescriptionChapters,
    parseCount, commentTime, findComment, commentLink, cmKey, parseYtComments, parseBiliComments, cleanComments, commentsToMarkdown,
    videoIdOf, videoKey, parseTimedTextXml, timeLink, sliceJson, parseYouTubePage, pickTrack, scriptOf, splitLong, resegmentLines, outlineToTree, wrapText, layoutMindmap, mindmapSvg, normalizeFlow, layoutFlow, flowSvg, FLOW_KINDS,
    searchLines, splitMarks, citations, lineAt, chapterAt, makeNote, addNote, trimNotes, lruTrim, trimEdits, sortNotes, searchNotes, baseKey, partOf, notesToMarkdown, studyDoc, contextAround, viewLine, toMarkdown, splitSummary, parsePoint, toSrt, toTxt, cardsToCsv, fileName, hashText, cacheKey, pruneCache, STYLES, REWRITES, SUGGESTIONS,
  }
})()
