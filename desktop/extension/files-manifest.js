// 脚本文件清单的唯一来源：后台脚本的加载顺序、各功能注入页面的文件顺序、文件之间的依赖。
// 其他地方（background.js 的 importScripts、features.js 的常驻脚本、bg-actions.js / bg-trans.js / bg-capture.js 的按需注入）
// 都从这里取，不再各写一份。只有 manifest.json 的 background.scripts 必须写成静态数组（Firefox 以它加载后台脚本），
// 由单元测试（extension-files.test.mjs）保证它和这里的 BG_SCRIPTS 一致。
//
// 这是普通全局脚本：后台 Service Worker 用 importScripts 加载，Firefox 用 manifest 的 background.scripts 加载，
// 扩展页面用 <script> 加载，单元测试用 vm 加载。不要在这里用 import / export。

const ccFiles = (() => {
  const UI = 'content/ui.js'
  const VDOCK = 'content/vdock.js'

  // 后台脚本，按加载顺序（不含 background.js：它最后加载，manifest 里排在最末）。files-manifest.js 自己排第一。
  const BG_SCRIPTS = [
    'files-manifest.js',
    'shared.js',
    'perms.js',
    'features.js',
    'clean-link.js',
    'progress.js',
    'takeover.js',
    'sniff.js',
    'media-sniff.js',
    'naming.js',
    'engines.js',
    'later.js',
    'capture-plan.js',
    'trans-logic.js',
    'bg-guard.js',
    'bg-defer.js',
    'bg-trans.js',
    'bg-link.js',
    'bg-capture.js',
    'bg-vcache.js',
    'bg-actions.js',
  ]
  const BG_MAIN = 'background.js'

  // 注入网页的文件组：key 是功能名，值是按顺序加载的文件（先加载的在前面）。
  const GROUPS = {
    float: [UI, VDOCK, 'content/float-button.js'],
    zoom: [UI, 'content/original.js', 'content/hover-zoom.js'],
    audio: [UI, VDOCK, 'content/audio-logic.js', 'content/audio-chain.js', 'content/audio-fx.js'],
    video: [UI, VDOCK, 'content/video-logic.js', 'content/video-control.js'],
    trans: [UI, 'trans-logic.js', 'content/webtrans.js'],
    selbar: [UI, 'selbar-logic.js', 'content/wordcard.js', 'content/selbar.js'],
    write: [UI, 'writing-logic.js', 'content/writing.js'],
    unlock: ['content/unlock.js'],
    bypass: ['bypass.js'],
    stub: ['content/stub.js'],
    vsum: [UI, VDOCK, 'vsum-logic.js', 'trans-logic.js', 'learn-logic.js', 'content/wordcard.js', 'content/vsum-source.js', 'content/vsession.js', 'content/vsum-style.js', 'content/vsum-transcript.js', 'content/vsum-notes.js', 'content/vsum-comments.js', 'content/vsum-vocab.js', 'content/vsum-mindmap.js', 'content/vsum-more.js', 'content/vsum-dock.js', 'content/vsum-util.js', 'content/vsum-session.js', 'content/vsum-flow.js', 'content/vsum-local.js', 'content/vsum-tabs.js', 'content/vsum.js'],
    subLayer: [UI, VDOCK, 'vsum-logic.js', 'sublayer-logic.js', 'learn-logic.js', 'content/vsum-source.js', 'content/vsession.js', 'content/wordcard.js', 'content/sublayer.js'],
    shot: [UI, 'trans-logic.js', 'content/shot.js'],
    picker: [UI, 'content/picker.js'],
    excludePick: [UI, 'content/excludepick.js'],
    area: [UI, 'content/area.js'],
    collect: ['content/collect.js'],
    extract: ['content/extract.js'],
  }

  // 显式的依赖表：文件 → 必须排在它前面的文件（读它的头部 / 加载时就要用的全局对象）。
  // 单元测试检查每个文件组都满足这张表，并扫描源码里的 globalThis.ccXxx，提醒这张表漏了什么。
  const REQUIRES = {
    'content/vdock.js': [UI],
    'content/float-button.js': [UI, VDOCK],
    'content/hover-zoom.js': [UI, 'content/original.js'],
    'content/audio-chain.js': ['content/audio-logic.js'],
    'content/audio-fx.js': [UI, VDOCK, 'content/audio-logic.js', 'content/audio-chain.js'],
    'content/video-control.js': [UI, VDOCK, 'content/video-logic.js'],
    'content/webtrans.js': [UI, 'trans-logic.js'],
    'content/wordcard.js': [UI],
    'content/selbar.js': [UI, 'selbar-logic.js', 'content/wordcard.js'],
    'content/writing.js': [UI, 'writing-logic.js'],
    'content/vsum-source.js': [UI, 'vsum-logic.js'],
    'content/vsession.js': [UI, 'vsum-logic.js', 'content/vsum-source.js'],
    'content/vsum-style.js': [],
    'content/vsum-transcript.js': [UI, 'learn-logic.js', 'content/vsession.js', 'content/wordcard.js'],
    'content/vsum-notes.js': [],
    'content/vsum-comments.js': ['content/vsum-source.js'],
    'content/vsum-vocab.js': [],
    'content/vsum-mindmap.js': [],
    'content/vsum-more.js': [],
    'content/vsum-util.js': [UI, 'trans-logic.js'],
    'content/vsum-session.js': [UI],
    'content/vsum-flow.js': [],
    'content/vsum-local.js': [],
    'content/vsum-tabs.js': [],
    'content/vsum-dock.js': [VDOCK],
    'content/vsum.js': [UI, VDOCK, 'vsum-logic.js', 'trans-logic.js', 'content/vsum-source.js', 'content/vsession.js', 'content/vsum-style.js', 'content/vsum-dock.js', 'content/vsum-transcript.js', 'content/vsum-notes.js', 'content/vsum-comments.js', 'content/vsum-vocab.js', 'content/vsum-mindmap.js', 'content/vsum-more.js', 'content/vsum-util.js', 'content/vsum-session.js', 'content/vsum-flow.js', 'content/vsum-local.js', 'content/vsum-tabs.js'],
    'content/sublayer.js': [UI, VDOCK, 'vsum-logic.js', 'sublayer-logic.js', 'learn-logic.js', 'content/vsession.js', 'content/wordcard.js'],
    'content/shot.js': [UI],
    'content/picker.js': [UI],
    'content/excludepick.js': [UI],
    'content/area.js': [UI],
  }

  return { BG_SCRIPTS, BG_MAIN, GROUPS, REQUIRES, ui: UI }
})()
