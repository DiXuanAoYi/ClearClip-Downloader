// 写作窗“帮我回”：标签归属、聊天程序识别、请求内容、还差什么、幽默提醒、看懂了。
import assert from 'node:assert/strict'
import * as W from '../../src/writePresets.ts'

// 四个标签：原来的动作各归其位
assert.equal(W.tabOf('reply'), 'reply')
assert.equal(W.tabOf('polite'), 'rewrite')
assert.equal(W.tabOf('grammar'), 'check')
assert.equal(W.tabOf('translate'), 'more')
assert.equal(W.tabOf('nope'), 'rewrite')
for (const ids of Object.values(W.TAB_ACTIONS)) for (const id of ids) assert.ok(W.ACTIONS.some((a) => a.id === id), id)

// 聊天 / 邮件程序
assert.deepEqual(W.appInfo('WeChat.exe'), { name: '微信', chat: true, email: false })
assert.deepEqual(W.appInfo('C:\\Program Files\\Feishu\\Feishu.exe'), { name: '飞书', chat: true, email: false })
assert.equal(W.appInfo('OUTLOOK.EXE').email, true)
assert.deepEqual(W.appInfo('winword.exe'), { name: 'winword', chat: false, email: false })
assert.equal(W.relationKey('Weixin.exe', '王经理'), 'weixin.exe|王经理')
assert.equal(W.relationKey('weixin.exe', ''), 'weixin.exe')

// 请求：只带当前意思用得上的东西
const f = { ...W.newReplyForm(), relation: 'superior', intent: 'conditional', detail: ' 下周一调休一天 ', alt: 'another_time', questions: [{ text: 'x', on: true }], extra: '9 点准时到' }
let r = W.buildReplyRequest(f, '周六能来加班吗？', 'weixin.exe')
assert.equal(r.kind, 'write')
assert.equal(r.write.action, 'reply')
assert.equal(r.write.tone, 'eq', '默认高情商')
assert.equal(r.write.candidates, 3, '写作窗一次三版')
assert.deepEqual(r.write.reply, { relation: 'superior', intent: 'conditional', detail: '下周一调休一天', alt: '', questions: [], context: '', email: false })
assert.equal(r.write.extra, '9 点准时到')
r = W.buildReplyRequest({ ...f, intent: 'ask', questions: [{ text: '指哪份方案', on: true }, { text: '几点前', on: false }, { text: ' ', on: true }] }, '方案明天给我', 'outlook.exe')
assert.deepEqual(r.write.reply.questions, ['指哪份方案'])
assert.equal(r.write.reply.detail, '')
assert.equal(r.write.reply.email, true)
r = W.buildReplyRequest({ ...f, intent: 'decline', detail: '', alt: 'someone_else' }, '帮个忙？')
assert.equal(r.write.reply.alt, 'someone_else')
const t = W.buildTweakRequest('softer', '可以。', '周六能来吗？', 'qq.exe')
assert.deepEqual(t.write.reply, { tweak: 'softer', received: '周六能来吗？', email: false })
assert.deepEqual(W.buildReadRequest('消息', ' 上文 ').write.reply, { context: '上文' })

// 还差什么才能生成
assert.equal(W.replyMissing(W.newReplyForm(), ' '), '先把收到的消息放进来')
assert.equal(W.replyMissing({ ...W.newReplyForm(), intent: 'conditional' }, '消息'), '写上条件')
assert.equal(W.replyMissing({ ...W.newReplyForm(), intent: 'ask', questions: [{ text: 'a', on: false }] }, '消息'), '勾选或写上要问的问题')
assert.equal(W.replyMissing({ ...W.newReplyForm(), intent: 'agree' }, '消息'), '')

// 小框：有条件必填，答应和问清楚没有
assert.equal(W.detailField('conditional').required, true)
assert.equal(W.detailField('decline').label, '理由')
assert.equal(W.detailField('agree'), null)
assert.equal(W.detailField('ask'), null)

// 幽默接话：对上级、客户提醒
assert.equal(W.humorRisk({ relation: 'superior', tone: 'banter' }), true)
assert.equal(W.humorRisk({ relation: 'friend', tone: 'banter' }), false)
assert.equal(W.humorRisk({ relation: 'client', tone: 'eq' }), false)

// 看懂了
assert.deepEqual(W.readParts({ ask: '请你周六加班', facts: '时间已给出', unclear: [], relation: '', who: '' }), { ask: '请你周六加班', rest: '，时间已给出。' })
assert.equal(W.readParts({ ask: '', facts: '', unclear: [], relation: '', who: '' }), null)
assert.equal(W.readParts(null), null)

// 改写润色带上“怎么说”，检查不带
assert.equal(W.buildWriteRequest({ action: W.actionById('polite'), text: 'x', tone: 'banter' }).write.tone, 'banter')
assert.equal(W.buildWriteRequest({ action: W.actionById('grammar'), text: 'x', tone: 'banter' }).write.tone, 'auto')

console.log('write-reply: ok')
