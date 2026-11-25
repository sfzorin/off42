import {MARK_ALL} from '@/lib/compliance/marks.ts';
import {getComplianceMarkCopy, getComplianceCategoryLabel, getComplianceRegionLabel} from '@/lib/compliance/localization.ts';
const out:any[]=[];
for(const [id,m] of Object.entries(MARK_ALL) as any){
  const copy:any={}; for(const l of ['en','ru','tr','de','fr','es','it','zh-Hans','zh-Hant','ko','ja']) copy[l]={...getComplianceMarkCopy(l as any,id as any,m), category:getComplianceCategoryLabel(l as any,m.category), region:getComplianceRegionLabel(l as any,m.region)};
  out.push({id,...m,copy});
}
console.log(JSON.stringify(out,null,1));
