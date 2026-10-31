/**
 * 每日一问：同一天展示同一个问题，答案按日期保存。
 * 输入防抖写入 configJson；答案只保留最近 120 天。
 */

import {
  cardMeta,
  createCardShell,
  createTextarea,
  createStatus,
  createDebouncedCommit,
  dailyIndex,
  dayKey,
  pruneDayMap,
  capText,
} from '../../space-kit/index.js';

const KEY = 'widget/dailyQuestion';
const KEEP_DAYS = 120;
const MAX_ANSWER = 2000;

const QUESTIONS = {
  zh: [
    '今天什么事值得你记住？',
    '你在哪件事上投入了最多注意力？',
    '今天学到的一个新东西是什么？',
    '如果只能完成一件事，你会选哪件？',
    '今天哪个决定可以做得更好？',
    '谁今天帮助了你？你如何回应？',
    '什么让你精力充沛？什么在消耗你？',
    '你今天避开了哪个重要但不舒服的对话？',
    '如果明天重来一次，你会改变什么？',
    '今天有什么小进步值得肯定？',
    '你现在最担心什么？它可控吗？',
    '今天你对自己的认识多了哪一点？',
  ],
  en: [
    'What deserves to be remembered from today?',
    'Where did most of your attention go today?',
    'What is one new thing you learned today?',
    'If you could finish only one thing, which would it be?',
    'Which decision today could have been better?',
    'Who helped you today, and how did you respond?',
    'What gave you energy today, and what drained it?',
    'Which important but uncomfortable conversation did you avoid?',
    'If tomorrow repeated today, what would you change?',
    'What small progress today is worth acknowledging?',
    'What worries you most right now? Is it within your control?',
    'What did you learn about yourself today?',
  ],
};

export const dailyQuestionWidget = {
  key: KEY,
  name: 'Daily Question',
  defaultData: { answers: {} },

  render(container, data = {}, display = {}, { t = (key, fallback) => fallback || key, lang = 'zh_CN', onDataChange } = {}) {
    const today = dayKey();
    const list = QUESTIONS[lang === 'en' ? 'en' : 'zh'];
    const question = list[dailyIndex(today, list.length)];
    const answers = data.answers && typeof data.answers === 'object' && !Array.isArray(data.answers)
      ? data.answers
      : {};

    const shell = createCardShell({
      title: t('dailyQuestionTitle', '每日一问'),
      icon: cardMeta(KEY)?.icon || 'help',
      className: 'space-card-daily-question',
    });
    container.replaceChildren(shell.root);

    const questionEl = document.createElement('p');
    questionEl.className = 'space-question';
    questionEl.textContent = question;

    const status = createStatus(t('dailyQuestionSaved', '已保存'));
    const commit = createDebouncedCommit((value) => {
      const nextAnswers = pruneDayMap({ ...answers, [today]: value }, KEEP_DAYS, today);
      onDataChange?.({ ...data, answers: nextAnswers });
      status.set(t('dailyQuestionSaved', '已保存'));
    }, 600);

    const textarea = createTextarea({
      value: answers[today] || '',
      placeholder: t('dailyQuestionPlaceholder', '写下你的答案…'),
      rows: 4,
      maxLength: MAX_ANSWER,
      ariaLabel: question,
      onInput: (value) => {
        status.set(t('scratchpadSaving', '保存中…'));
        commit.schedule(capText(value, MAX_ANSWER));
      },
      onBlur: () => commit.flush(),
    });

    shell.body.append(questionEl, textarea, status.node);
    shell.setFooter(today);

    const flushOnHide = () => commit.flush();
    const flushWhenHidden = () => {
      if (document.hidden) commit.flush();
    };
    window.addEventListener('pagehide', flushOnHide);
    document.addEventListener('visibilitychange', flushWhenHidden);

    return () => {
      window.removeEventListener('pagehide', flushOnHide);
      document.removeEventListener('visibilitychange', flushWhenHidden);
      commit.flush();
      container.replaceChildren();
    };
  },

  renderSettings(container, data = {}, onChange = () => {}, { t = (key, fallback) => fallback || key } = {}) {
    container.replaceChildren();
    const wrap = document.createElement('div');
    wrap.className = 'inspector-field-group';
    wrap.innerHTML = `
      <div class="inspector-notice">${t('dailyQuestionHint', '答案按日期保存，只保留最近 120 天。')}</div>
      <button type="button" class="danger" id="dq-clear">${t('dailyQuestionClear', '清空今天的答案')}</button>
    `;
    wrap.querySelector('#dq-clear').onclick = () => {
      const today = dayKey();
      const next = { ...(data.answers || {}) };
      delete next[today];
      onChange({ ...data, answers: next });
    };
    container.append(wrap);
  },

  styles: '',
};
