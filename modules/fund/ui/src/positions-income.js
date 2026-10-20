(() => {
const { t, decimalToMinor, formatMinorExact } = globalThis.Fund;
const esc = value => String(value ?? "").replace(/[&<>"']/g,c=>({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"})[c]);
// 卡片主指标与可展开详情共用金额、比例、日期及缺值口径。
function factFor(holding,key){const confirmed=holding.income?.[key],estimate=holding.income?.estimatedDay;return key==='day'&&confirmed?.amount==null&&estimate?.date===confirmed?.date&&(estimate?.amount!=null||estimate?.rateBps!=null)?estimate:confirmed;}
function inline(holding, key) {
  const mode=Fund.home?.displayMode() || "hidden";
  const fact=factFor(holding,key);
  const gap = fact?.reason==="missing_daily_price" ? t("fundLedgerIncomeAwaiting") : fact?.reason==="unknown_cost" ? t("fundLedgerCostUnknown") :
    fact?.reason==="missing_rate_basis" ? t("fundLedgerIncomeNoRateBasis") : t("fundLedgerIncomeMissing");
  let amount=fact?.amount==null?null:decimalToMinor(fact.amount);
  let rate=fact?.rateBps==null?null:BigInt(fact.rateBps);
  let date=fact?.date;


  const sign=mode==="hidden"?0:mode==="percent"?(rate??0n):(amount??0n);
  const color=sign>0?"up":sign<0?"down":"";
  let meta="";
  if(key==="day"||key==="yesterday") {
    meta=/^\d{4}-\d{2}-\d{2}$/.test(date||"")?
      t("fundLedgerIncomeDate",{month:Number(date.slice(5,7)),day:Number(date.slice(8,10))}):t("fundLedgerIncomeDateUnknown");
  } else if(key==="holding") {
    meta=holding.holdingDays==null?t("fundLedgerIncomeAgeUnknown"):t("fundLedgerIncomeHeldDays",{n:holding.holdingDays});
  }
  if(fact?.estimated&&mode!=='hidden')meta+=' · '+t('fundLedgerIncomeEstimated',{coverage:(Number(fact.coverageBps||0)/100).toFixed(2)});
  const prefix = sign > 0 ? "+" : "";
  return '<span class="'+color+'">'+
    (mode!=="amount"?"****":amount==null?"–":prefix+esc(formatMinorExact(amount)))+'</span><small class="'+color+'">'+
    (mode==="hidden"?"****":rate==null?"–":prefix+esc(formatMinorExact(rate))+"%")+'</small>'+
    (meta?'<small class="pos-income-meta">'+esc(meta)+'</small>':"")+
    (mode!=="hidden"&&amount==null&&rate==null?'<small class="pos-income-gap">'+esc(gap)+'</small>':"");
}
Fund.positionIncome={inline,fact:factFor,label:(holding,key,fallback)=>factFor(holding,key)?.estimated?'fundLedgerEstimatedDay':fallback};
})();
