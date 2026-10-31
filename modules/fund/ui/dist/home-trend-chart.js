// 资产走势绘制基元：单调平滑曲线与紧凑金额轴（自 home-overview 拆分，独立行数合规）。
(() => {
const trend = globalThis.Fund.homeTrend = globalThis.Fund.homeTrend || {};
function smoothPath(pts){
  if(pts.length===0)return "";
  if(pts.length===1)return `M${pts[0][0]},${pts[0][1]}`;
  if(pts.length===2)return `M${pts[0][0]},${pts[0][1]}L${pts[1][0]},${pts[1][1]}`;
  const n=pts.length,xs=pts.map(p=>p[0]),ys=pts.map(p=>p[1]);
  const dx=[],m=[],t=new Array(n);
  for(let i=0;i<n-1;i++){dx[i]=xs[i+1]-xs[i];m[i]=(ys[i+1]-ys[i])/(dx[i]||1);}
  t[0]=m[0];t[n-1]=m[n-2];
  for(let i=1;i<n-1;i++)t[i]=m[i-1]*m[i]<=0?0:(m[i-1]+m[i])/2;
  for(let i=0;i<n-1;i++){
    if(m[i]===0){t[i]=0;t[i+1]=0;continue;}
    const a=t[i]/m[i],b=t[i+1]/m[i],s=a*a+b*b;
    if(s>9){const tau=3/Math.sqrt(s);t[i]=tau*a*m[i];t[i+1]=tau*b*m[i];}
  }
  let d=`M${pts[0][0]},${pts[0][1]}`;
  for(let i=0;i<n-1;i++)d+=`C${(xs[i]+dx[i]/3).toFixed(1)},${(ys[i]+t[i]*dx[i]/3).toFixed(1)} ${(xs[i+1]-dx[i]/3).toFixed(1)},${(ys[i+1]-t[i+1]*dx[i]/3).toFixed(1)} ${xs[i+1]},${ys[i+1]}`;
  return d;
}
// 紧凑单位只缩放金额，不能把相邻刻度舍入成同一个值。
function fmtAxisAmount(value, language) {
  return value.toLocaleString(language, { notation: "compact", maximumFractionDigits: 10 });
}
trend.smoothPath = smoothPath;
trend.fmtAxisAmount = fmtAxisAmount;
})();
