// 账本 V2 的 AI 评估：模块组装输入 → 宿主 broker 推理 → 回存结果。
//
// 为什么不在这里发 HTTP：模型凭据只在 Keychain，推理由 Extension 侧
// module-inference-broker 发起（ADR-0037 / ADR-0044）。模块只能通过
// postMessage 请求，拿不到也存不了 Key。
//
// 复用已有 broker 封装 Fund.home.requestCapability（home-evidence.js 暴露），
// 它已经处理了 generation/challenge 校验、超时和取消。
(function () {
  "use strict";
  if (!window.Fund) return;
  const { api, t } = Fund;

  const PROMPT_VERSION = "v1";
  const MAX_TOKENS = 900;
  const POLL_MS = 800;
  const POLL_LIMIT = 120; // 约 96 秒

  const SYSTEM_RULES = [
    "你是个人投资记账助手，只做风险与结构提示。",
    "",
    "必须遵守：",
    "1. 只基于提供的数据陈述，数据缺失就明说，不臆测。",
    "2. 不预测涨跌，不给「买入/卖出/加仓/减仓/止损」这类操作指令。",
    "3. 不评价用户决策的对错，不做情绪化表述。",
    "4. 全部使用简体中文。",
    "",
    "严格只输出下面这个 JSON，不要输出任何其他内容：",
    "{",
    '  "score": <0-100 整数，持仓健康度>,',
    '  "tone": "<steady|watch|alert>",',
    '  "label": "<8 字以内的定调>",',
    '  "summary": "<80 字以内的说明>",',
    '  "concerns": ["<最多 3 条，每条 20 字以内>"]',
    "}",
  ].join("\n");

  function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  /** 从模型输出里提取 JSON：兼容 ```json 围栏和前后多余说明。 */
  function parseResult(text) {
    if (typeof text !== "string" || !text.trim()) {
      throw Error(t("fundLgAiEmpty"));
    }
    const fenced = text.match(/```(?:json)?\s*([\s\S]*?)```/i);
    const candidate = fenced ? fenced[1] : text;
    const start = candidate.indexOf("{");
    const end = candidate.lastIndexOf("}");
    if (start < 0 || end <= start) throw Error(t("fundLgAiUnparsable"));
    const raw = JSON.parse(candidate.slice(start, end + 1));

    const score = Number(raw.score);
    if (!Number.isFinite(score)) throw Error(t("fundLgAiUnparsable"));
    const tone = ["steady", "watch", "alert"].includes(raw.tone) ? raw.tone : "watch";
    const label = String(raw.label || "").trim().slice(0, 40);
    const summary = String(raw.summary || "").trim().slice(0, 300);
    const concerns = Array.isArray(raw.concerns)
      ? raw.concerns.map((c) => String(c).trim()).filter(Boolean).slice(0, 3)
      : [];
    if (!label || !summary) throw Error(t("fundLgAiUnparsable"));
    return {
      score: Math.max(0, Math.min(100, Math.round(score))),
      tone,
      label,
      summary,
      concerns,
    };
  }

  function buildPrompt(context) {
    return [
      SYSTEM_RULES,
      "",
      "以下是这只持仓的实际情况（金额单位与币种见字段）：",
      JSON.stringify(context, null, 2),
    ].join("\n");
  }

  /** 等模型跑完：start 之后轮询 status，直到 done 或超时。 */
  async function waitForDone(broker) {
    for (let i = 0; i < POLL_LIMIT; i += 1) {
      await sleep(POLL_MS);
      const status = await broker("status");
      if (status && status.done) return status;
    }
    throw Error(t("fundLgAiTimeout"));
  }

  /**
   * 对一只持仓做评估并保存。
   * @param {{account_id:number,instrument_id:number}} holding
   * @param {(phase:string)=>void} [onProgress]
   */
  async function evaluate(holding, onProgress) {
    const broker = Fund.home?.requestCapability;
    if (typeof broker !== "function") throw Error(t("fundLgAiUnavailable"));

    onProgress && onProgress("context");
    const context = await api(
      "GET",
      `/api/v2/ai/context?account=${holding.account_id}&instrument=${holding.instrument_id}`
    );

    onProgress && onProgress("options");
    const options = await broker("options");
    const model = options && options.defaultModel;
    if (!model || model.unavailable || !model.configToken) {
      throw Error(t("fundLgAiNoModel"));
    }

    onProgress && onProgress("running");
    const text = buildPrompt(context);
    await broker("start", {
      maxTokens: MAX_TOKENS,
      selection: "default",
      expectedConfigToken: model.configToken,
      text,
    });
    const status = await waitForDone(broker);
    const parsed = parseResult(status.text !== undefined ? status.text : status.result);

    onProgress && onProgress("saving");
    const saved = await api("POST", "/api/v2/ai/evaluations", {
      account_id: holding.account_id,
      instrument_id: holding.instrument_id,
      provider_id: (model.providerId || "").slice(0, 128),
      model_id: (model.modelId || "").slice(0, 256),
      score: parsed.score,
      tone: parsed.tone,
      label: parsed.label,
      summary: parsed.summary,
      concerns: parsed.concerns,
      snapshot_json: JSON.stringify({ promptVersion: PROMPT_VERSION, context }),
    });
    if (onProgress) onProgress("done");
    return saved;
  }

  function latest(accountId, instrumentId) {
    return api(
      "GET",
      `/api/v2/ai/evaluations?account=${accountId}&instrument=${instrumentId}`
    );
  }

  function trend(accountId, instrumentId, limit) {
    return api(
      "GET",
      `/api/v2/ai/trend?account=${accountId}&instrument=${instrumentId}&limit=${limit || 30}`
    );
  }

  Fund.ledgerAI = { evaluate, latest, trend, parseResult };
})();
