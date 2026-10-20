// 单条持仓来源编辑：读取可取消，写入复用期初/交易更正及其版本与审计。
(() => {
const {actions,api,t}=Fund;
let current=null,request=null,generation=0;
const esc=value=>Fund.home.escape(value);
function stop() {generation++;request?.abort();request=null;}
actions.leaveHoldingEditor=()=>{stop();if(current?.open)Fund.home.closePanel();current=null;};
actions.openHoldingEditor=holding=>{
  const home=Fund.home;if(home.sensitive())return;
  actions.leaveHoldingEditor();
  let page=1,model=null,error="";
  const html=()=>'<p>'+esc(holding.account)+' · '+esc(holding.fundName||holding.fundCode)+'</p>'+
    '<p>'+esc(t("fundLedgerEditCurrent",{quantity:holding.quantity,cost:holding.costUnknown?t("fundLedgerCostUnknown"):holding.cost}))+'</p>'+
    '<p>'+esc(t("fundLedgerEditSourceHint"))+'</p><div class="pos-records-list">'+
    (error?'<p class="err" role="status">'+esc(error)+'</p><button type="button" data-record-retry>'+esc(t("fundLedgerRetry"))+'</button>':
     !model?'<p role="status">'+esc(t("fundHomeLoading"))+'</p>':
     (model.opening?'<article><b>'+esc(t("fundHomeOpeningHolding"))+'</b><p>'+esc(model.opening.asOf)+' · '+
       esc(t("fundLedgerLabelQuantity"))+' '+esc(model.opening.quantity)+' · '+esc(t("fundHomeCostOptional"))+' '+
       esc(model.opening.totalCost??t("fundLedgerCostUnknown"))+'</p><button type="button" data-record-opening>'+
       esc(t("fundLedgerEditOpening"))+'</button></article>':"")+
     model.transactions.map(tx=>'<article><b>'+esc(t(tx.type==="BUY"?"fundLedgerTypeBuy":"fundLedgerTypeSell"))+
       ' · '+esc(tx.tradeDate)+'</b><p>'+esc(t("fundLedgerLabelQuantity"))+' '+esc(tx.quantity)+' · '+
       esc(t("fundLedgerLabelPrice"))+' '+esc(tx.price)+' · '+esc(t("fundLedgerLabelFee"))+' '+esc(tx.fee)+'</p>'+
       (tx.source==="transfer"?'<p>'+esc(t("fundLedgerEditTransferHint"))+'</p>':
        tx.state!=="confirmed"?'<p>'+esc(t("fundLedgerEditPendingHint"))+'</p>':
        '<button type="button" data-record-transaction="'+tx.id+'">'+esc(t("fundLedgerEditTransaction"))+'</button>')+'</article>').join("")+
     (!model.opening&&!model.transactions.length?'<p>'+esc(t("fundLedgerEditNoRecords"))+'</p>':"")+
     '<div class="investment-pagination"><button type="button" data-record-prev '+(page===1?"disabled":"")+'>'+esc(t("fundHomePrevious"))+
       '</button><span>'+page+'</span><button type="button" data-record-next '+(!model.hasMore?"disabled":"")+'>'+esc(t("fundHomeNext"))+'</button></div>')+'</div>';
  const bind=root=>{
    root.querySelector('[data-record-retry]')?.addEventListener("click",load);
    root.querySelector('[data-record-prev]')?.addEventListener("click",()=>{page--;load();});
    root.querySelector('[data-record-next]')?.addEventListener("click",()=>{page++;load();});
    root.querySelector('[data-record-opening]')?.addEventListener("click",()=>actions.editOpeningRecord(holding,model.opening));
    root.querySelectorAll('[data-record-transaction]').forEach(button=>button.onclick=()=>{
      const tx=model.transactions.find(row=>row.id===Number(button.dataset.recordTransaction));
      if(tx)actions.editTransactionRecord({...tx,returnFocusId:holding.returnFocusId});
    });
  };
  const owner=home.panel(()=>t("fundLedgerEditHolding"),html,bind);
  if(!owner)return;current=owner;
  owner.addEventListener("close",()=>{if(current===owner){stop();current=null;}},{once:true});
  function draw(){if(current!==owner||!owner.open)return;owner.querySelector(".home-manager-body").innerHTML=html();bind(owner);}
  async function load(){
    stop();const gen=generation,controller=request=new AbortController();model=null;error="";draw();
    try {
      const data=await api("GET","/api/holding/records?accountId="+holding.accountId+"&fundId="+holding.fundId+"&page="+page,undefined,{signal:controller.signal});
      if(gen!==generation||current!==owner||!owner.open)return;
      model=data;draw();
    } catch(e) {if(gen!==generation||e.name==="AbortError")return;error=e.message;draw();}
    finally {if(gen===generation)request=null;}
  }
  load();
};
window.addEventListener("pagehide",actions.leaveHoldingEditor);
document.addEventListener("visibilitychange",()=>{if(document.hidden)actions.leaveHoldingEditor();});
document.addEventListener("fund:privacy-changed",()=>{if(Fund.home.displayMode()!=="amount")actions.leaveHoldingEditor();});
})();
