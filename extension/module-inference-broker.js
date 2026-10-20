// ADR-0037：通用会话代理，不解释模块业务或提供凭据。
import {acquireModelApi,releaseModelApi} from './ai-usage-client/client.js';
export function createModuleInferenceBroker({context,acquire=acquireModelApi,release=releaseModelApi,openLink=url=>chrome.tabs.create({url}),openSettings}) {
 // mode/T06：openSettings 由页面装配入口注入（下层不回导上层设置页）；
 // 缺依赖在装配期明确失败，不静默无响应。
 if (typeof openSettings !== 'function') throw new Error('module-inference-broker: openSettings is required (inject from the page assembly entry)');
 let api=null,job=null,starting=false,epoch=0;
 const owner=crypto.randomUUID()+crypto.randomUUID();
 async function dispose(){epoch++;const a=api,j=job;api=null;job=null;starting=false;if(a&&j)try{await a.inferenceCancel({owner,id:j});}catch{}if(a)release();}
 async function handle(event){
  const c=context(),m=event.data;
  if(!m || !['natives:ai','natives:open-link','natives:model-settings'].includes(m.type))return false;
  if(event.source!==c.frame?.contentWindow || m.generation!==c.generation || m.challenge!==c.challenge || typeof m.requestId!=='string' || !/^[a-zA-Z0-9-]{1,128}$/.test(m.requestId))return true;
  const before=epoch;
  const reply=(result,error)=>{const n=context();if(epoch===before && n.frame===c.frame && n.generation===c.generation && n.challenge===c.challenge)c.frame.contentWindow.postMessage({type:'natives:reply',requestId:m.requestId,generation:c.generation,challenge:c.challenge,result,error},'*');};
  try {
   if(m.type==='natives:model-settings'){
    if(!c.capabilities.includes('ai.inference.request')||m.op!=='open')throw Error('配置能力未授权');
    await openSettings();reply({opened:true});return true;
   }
   if(m.type==='natives:open-link'){
    if(!c.capabilities.includes('system.link.open'))throw Error('外链能力未授权');
    if(typeof m.url!=='string'||m.url.length>2048)throw Error('链接无效');
    const u=new URL(m.url);if(!['https:','http:'].includes(u.protocol)||u.username||u.password)throw Error('链接无效');
    await openLink(u.href);reply({opened:true});return true;
   }
   if(!c.capabilities.includes('ai.inference.request'))throw Error('推理能力未授权');
   if(!['options','start','status','cancel'].includes(m.op))throw Error('推理方法无效');
   if(!api)api=acquire();
   if(m.op==='options'){reply(await api.inferenceOptions());return true;}
   if(m.op==='start'){
    if(job||starting)throw Error('已有分析正在运行');
    if(typeof m.text!=='string'||[...m.text].length>16000||!m.text.trim())throw Error('推理输入无效');
    // ADR-0037 / 方案11 §6：默认选择与显式模型分别严格校验，Broker 只转发，
    // 不解析默认模型（由 Model Host 权威解析）。
    let startParams;
    if(m.selection==='default'){
     if(typeof m.expectedConfigToken!=='string'||!m.expectedConfigToken||m.expectedConfigToken.length>256)throw Error('推理输入无效');
     startParams={owner,text:m.text,selection:'default',expectedConfigToken:m.expectedConfigToken};
    }else{
     if(typeof m.providerId!=='string'||typeof m.modelId!=='string'||m.providerId.length>128||m.modelId.length>256)throw Error('推理输入无效');
     startParams={owner,text:m.text,providerId:m.providerId,modelId:m.modelId};
    }
    if(m.maxTokens!==undefined){
     if(!Number.isInteger(m.maxTokens)||m.maxTokens<512||m.maxTokens>8192)throw Error('推理输入无效');
     startParams.maxTokens=m.maxTokens;
    }
    starting=true;let result;const client=api;
    try{result=await client.inferenceStart(startParams);}finally{starting=false;}
    if(epoch!==before){try{await client.inferenceCancel({owner,id:result.id});}catch{}return true;}
    job=result.id;
    // 返回实际采用的 providerId/modelId/route，供结果记录真实身份。
    reply({started:true,providerId:result.providerId,modelId:result.modelId,route:result.route});return true;
   }
   if(m.op==='cancel' && starting && !job){reply({cancelled:true});epoch++;return true;}
   if(!job)throw Error('没有运行中的分析');
   if(m.op==='cancel'){await api.inferenceCancel({owner,id:job});job=null;reply({cancelled:true});}
   else{const r=await api.inferenceStatus({owner,id:job});if(r.done)job=null;reply(r);}
  }catch(e){reply(null,e.message||'分析失败');}
  return true;
 }
 return {handle,dispose};
}
