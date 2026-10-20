// Focused scenario selection; the driver retains candidate and process lifecycle.
import { verifyNativeWatchlistGroups } from './watchlist-groups-native.mjs';
import { verifyNativeHomeHistory } from './home-history-native.mjs';
import { verifyNativeHomeRanges } from './home-range-native.mjs';
import { verifyNativeHoldingIncomeEdit } from './positions-income-edit-native.mjs';
import { verifyNativeTabRetention,verifyPositionsLoading } from './home-tab-retention-native.mjs';
import { verifyInvestmentRemediation,verifyStockDetails } from './investment-remediation-native.mjs';
import {verifyInlineAI} from './home-inline-ai-native.mjs';
import {verifyFilterFocus} from './home-filter-focus-native.mjs';
import {verifyInformationData} from './home-native-information.mjs';
import {verifyCurrentFundData} from './fund-current-native.mjs';
export async function dispatchNativeScenario({page,frame,context,report,fakeHome,out,env,extensionId}) {
  if(process.argv.includes('--fund-current')){report.scope='fund-current';await verifyCurrentFundData(page,frame,report);return true;}
  if(process.argv.includes('--positions-loading')){report.scope='positions-loading';await verifyPositionsLoading(page,context,frame,report);return true;}
  if(process.argv.includes('--information-data')){report.scope='information-data';await verifyInformationData(page,frame,report);return true;}
  if(process.argv.includes('--home-filter-focus')){report.scope='home-filter-focus';await verifyFilterFocus(page,frame,report);return true;}
  if(process.argv.includes('--stock-details')){report.scope='stock-details';await verifyStockDetails(page,frame,report);return true;}
  if(process.argv.includes('--inline-ai')){report.scope='inline-ai';await verifyInlineAI({page,frame,report,fakeHome,out,env,extensionId});return true;}
  if(process.argv.includes('--tab-retention')){report.scope='short-tab-retention';await verifyNativeTabRetention(page,context,report,extensionId);return true;}
  if(process.argv.includes('--trends-layout')){const {verifyTrendsLayout}=await import('../des/trends-layout-browser.test.mjs');report.scope='trends-layout';await verifyTrendsLayout(page,frame,page.frames().find(f=>f.url().startsWith('http://127.0.0.1:')),report,{native:true});return true;}
  if(process.argv.includes('--watchlist-groups')){report.scope='watchlist-groups';await verifyNativeWatchlistGroups(page,frame,report);return true;}
  if(process.argv.includes('--home-history')){report.scope='home-history';await verifyNativeHomeHistory(page,frame,report);return true;}
  if(process.argv.includes('--home-range')){report.scope='whole-asset-home-ranges';await verifyNativeHomeRanges({page,frame,report,fakeHome,out,env,extensionId:extensionId});return true;}
  if(process.argv.includes('--positions-income-edit')){report.scope='holding-income-edit';await verifyNativeHoldingIncomeEdit(page,frame,report);return true;}
  if(process.argv.includes('--investment-remediation')){report.scope='investment-remediation';await verifyInvestmentRemediation(page,frame,report);return true;}
  return false;
}
