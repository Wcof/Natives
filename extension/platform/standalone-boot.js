// ADR-0048 D5.2：DSH 用嵌入方式承载产品页面。嵌入时隐藏产品级聚合导航
// （其他模块入口、设置等），只保留当前模块的内容；数据面、会话与鉴权不变。
//
// 判定同时看两件事，任一成立即进入 standalone：
//   1. `?standalone=1`（Host 显式声明，兼容已有 URL）；
//   2. 文档被嵌入框架（`window.top !== window.self`）。
// 第二条让「进入模块后不再重复显示自身左侧侧边栏」不依赖查询串：旧 Host 进程、
// 手动嵌入或参数被剥离时同样生效。作为 head 阻塞脚本在首帧前打类，避免闪一下再隐藏；
// 页面 JS 用该类跳过无谓的菜单挂载。
try {
  const flagged = new URLSearchParams(location.search).get('standalone') === '1';
  let framed = false;
  try {
    framed = window.top !== window.self;
  } catch {
    framed = false;
  }
  if (flagged || framed) document.documentElement.classList.add('standalone');
} catch {}
