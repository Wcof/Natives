(() => {
// Public research series only; no holdings, cash, inference or persisted UI authority.
Fund.researchSeries = {
  minute(data) {
    if (!Array.isArray(data?.points)) throw Error('MINUTE_RESPONSE');
    const positive=value=>Number.isFinite(value)&&value>0?value:null;
    const date=String(data.date||'').replace(/^(\d{4})(\d{2})(\d{2})$/,'$1-$2-$3');
    return {...data,date,pre_close:positive(data.preClose),
      times:data.points.map(row=>row.time),
      prices:data.points.map(row=>positive(row.price)),
      avg_prices:data.points.map(row=>positive(row.vwap)),
      volumes:data.points.map(row=>Number.isFinite(row.volume)&&row.volume>=0?row.volume:null)};
  },
  kind(symbol, market, explicit) {
    if (explicit === 'fund' || market === 'jj' || /^jj/.test(symbol)) return 'fund';
    if (explicit === 'etf' || /^(sh(51|56|58)|sz(15|16))/.test(symbol)) return 'etf';
    return 'stock';
  },
  async nav(code, signal) {
    const rows = [], seen = new Set(); let cursor = null;
    for (let page = 0; page < 3; page++) {
      const response = await Fund.api('GET', '/api/nav?code=' + encodeURIComponent(String(code).replace(/^jj/,'')) + '&limit=100' + (cursor ? '&beforeDate=' + encodeURIComponent(cursor) : ''), null, { signal });
      for (const row of response.nav || []) if (!seen.has(row.navDate)) { rows.push(row); seen.add(row.navDate); }
      cursor = response.nextCursor; if (!cursor) break;
    }
    return rows.sort((a, b) => a.navDate.localeCompare(b.navDate));
  },
  reader({id,toolbar,controls=toolbar,rows,chart,keys,chartNodes}) {
    let selected=0,page=0;
    let dialog=document.getElementById(id+'-data-dialog');
    const readout=document.getElementById(id+'-chart-readout') || document.createElement('output');
    if(!readout.isConnected){readout.id=id+'-chart-readout';readout.className='chart-readout';readout.setAttribute('aria-live','polite');toolbar.append(readout);}
    if(!dialog){
      dialog=document.createElement('dialog');dialog.id=id+'-data-dialog';dialog.className='chart-data-dialog';
      const head=document.createElement('h2');head.textContent=Fund.t('fundChartData');head.id=id+'-data-title';head.dataset.i18n='fundChartData';dialog.setAttribute('aria-labelledby',head.id);
      const wrap=document.createElement('div');wrap.className='chart-data-scroll';const table=document.createElement('table'),thead=document.createElement('thead'),tbody=document.createElement('tbody');
      thead.id=id+'-data-head';tbody.id=id+'-data-body';table.append(thead,tbody);wrap.append(table);
      dialog.append(head,wrap);document.body.append(dialog);
      for(const [action,key] of [['close','fundHomeClose'],['prev','fundHomePrevious'],['next','fundHomeNext']]){const b=document.createElement('button');b.type='button';b.dataset.chartAction=action;b.dataset.reader=id;b.textContent=Fund.t(key);b.dataset.i18n=key;dialog.append(b);}
      const status=document.createElement('span');status.id=id+'-data-page';dialog.append(status);
      const controlRow=document.createElement('div');controlRow.className='chart-access-controls';toolbar.append(controlRow);
      for(const [action,key] of [['table','fundChartData'],['in','fundChartZoomIn'],['out','fundChartZoomOut'],['left','fundChartEarlier'],['right','fundChartLater'],['reset','fundChartReset']]){const b=document.createElement('button');b.type='button';b.className='tb-btn';b.dataset.chartAction=action;b.dataset.reader=id;b.textContent=Fund.t(key);b.dataset.i18n=key;controlRow.append(b);}
    }else for(const b of [...controls.querySelectorAll('[data-chart-action]'),...dialog.querySelectorAll('[data-chart-action]')])b.dataset.reader=id;
    const label=key=>({unitNav:'NAV',source:Fund.t('fundTrendsSeriesSource'),fetchedAt:Fund.t('fundTrendsFetchedAt'),price:Fund.t('fundTrendsPrice')})[key] || Fund.t('fundChart'+key);
    function read(index){const data=rows();if(!data.length)return;selected=Math.max(0,Math.min(data.length-1,index));const row=data[selected];readout.textContent=keys().map(key=>label(key)+': '+(row[key]??'–')).join(' · ');chart()?.dispatchAction({type:'showTip',seriesIndex:0,dataIndex:selected});}
    for(const node of chartNodes){if(!node)continue;node.tabIndex=0;node.setAttribute('aria-label',Fund.t('fundChartKeyboard'));node.addEventListener('keydown',event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key)||event.altKey||event.ctrlKey||event.metaKey)return;event.preventDefault();read(event.key==='Home'?0:event.key==='End'?rows().length-1:selected+(event.key==='ArrowLeft'?-1:1));});}
    function render(){const data=rows(),size=100;page=Math.max(0,Math.min(page,Math.max(0,Math.ceil(data.length/size)-1)));const head=document.createElement('tr');
      for(const key of keys()){const th=document.createElement('th');th.scope='col';th.textContent=label(key);head.append(th);}document.getElementById(id+'-data-head').replaceChildren(head);
      const body=document.getElementById(id+'-data-body');body.replaceChildren();
      for(const row of data.slice(page*size,(page+1)*size)){const tr=document.createElement('tr');for(const key of keys()){const td=document.createElement('td');td.textContent=row[key]??'–';tr.append(td);}body.append(tr);}
      document.getElementById(id+'-data-page').textContent=(page+1)+' / '+Math.max(1,Math.ceil(data.length/size))+' · '+data.length;
      dialog.querySelector('[data-chart-action=prev]').disabled=page===0;dialog.querySelector('[data-chart-action=next]').disabled=(page+1)*size>=data.length;
    }
    document.addEventListener('click',event=>{const button=event.target.closest('[data-chart-action]');if(button?.dataset.reader!==id)return;const action=button.dataset.chartAction;
      if(action==='close'){dialog.close();return;}if(action==='prev'||action==='next'){page+=action==='prev'?-1:1;render();return;}if(action==='table'){page=0;render();dialog.showModal();return;}
      const active=chart(),count=rows().length;if(!active||!count)return;const zoom=active.getOption().dataZoom?.[0]||{start:0,end:100},width=zoom.end-zoom.start,center=(zoom.start+zoom.end)/2;let start=zoom.start,end=zoom.end;
      if(action==='reset'){start=0;end=100;}else if(action==='in'||action==='out'){const next=Math.max(100/count,Math.min(100,width*(action==='in'?0.8:1.25)));start=Math.max(0,Math.min(100-next,center-next/2));end=start+next;}else{start=Math.max(0,Math.min(100-width,start+(action==='left'?-width:width)*0.2));end=start+width;}
      active.dispatchAction({type:'dataZoom',start,end});
    });
    const reset=()=>{selected=0;if(dialog.open)dialog.close();readout.textContent='';};
    document.addEventListener('fund:language-changed',()=>{for(const node of chartNodes)node?.setAttribute('aria-label',Fund.t('fundChartKeyboard'));if(dialog.open)render();if(readout.textContent)read(selected);});
    return {reset};
  },
  capture(identity, rows, chart, source) {
    const zoom=chart?.getOption().dataZoom?.[0] || {start:0,end:100};
    const start=Math.max(0,Math.floor(zoom.start/100*rows.length)),end=Math.min(rows.length,Math.ceil(zoom.end/100*rows.length));
    const selected=rows.slice(start,end).slice(-60).map(row=>Object.fromEntries(['date','open','high','low','close','price','volume','unitNav'].filter(key=>row[key]!==undefined).map(key=>[key,row[key]])));
    return {identity:{...identity,range:{start:zoom.start,end:zoom.end}},rows:selected,source:source || 'unknown',capturedAt:new Date().toISOString()};
  },
  navRows(rows, period) {
    if (!['week', 'month'].includes(period)) return rows;
    const groups = new Map();
    for (const row of rows) {
      const day = new Date(row.navDate + 'T00:00:00Z');
      if (!Number.isFinite(day.getTime())) continue;
      if (period === 'week') day.setUTCDate(day.getUTCDate() - (day.getUTCDay() + 6) % 7);
      const key = period === 'month' ? row.navDate.slice(0, 7) : day.toISOString().slice(0, 10);
      groups.set(key, row); // Actual last published NAV in each bucket, with its actual business date.
    }
    return [...groups.values()];
  },
  navOption(rows) {
    const color = Fund.themeColor;
    return { backgroundColor: 'transparent', animation: false, grid: { left: 56, right: 24, top: 24, bottom: 40 },
      title: { text: rows.length ? '' : Fund.t('fundTrendsNavEmpty'), left: 'center', top: 'center', textStyle: { color: color('--chart-text-dim'), fontSize: 13 } },
      xAxis: { type: 'category', data: rows.map(r => r.navDate), axisLabel: { color: color('--chart-text-dim') }, axisLine: { lineStyle: { color: color('--chart-axis') } } },
      yAxis: { type: 'value', scale: true, name: 'NAV', nameTextStyle: { color: color('--chart-text-dim') }, axisLabel: { color: color('--chart-text-dim') }, splitLine: { lineStyle: { color: color('--chart-grid') } } },
      dataZoom: [{ type: 'inside' }, { type: 'slider', height: 18, bottom: 0 }],
      series: [{ name: 'NAV', type: 'line', connectNulls: false, showSymbol: false, lineStyle: { color: color('--natives-accent'), width: 2 }, data: rows.map(r => /^\d+(\.\d+)?$/.test(r.unitNav) && Number.isFinite(Number(r.unitNav)) ? Number(r.unitNav) : null) }],
      tooltip: { trigger: 'axis', backgroundColor: color('--natives-surface'), textStyle: { color: color('--chart-text') } },
    };
  },
};
})();
