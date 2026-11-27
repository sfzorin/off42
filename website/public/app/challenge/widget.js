// Isolate the verification script from the application bundle. The frame exchanges only a challenge token.
const params = new URLSearchParams(location.search);
const report = (token) => parent.postMessage({off42Challenge:token},location.origin);
window.off42Ready = () => {
  try {
    turnstile.render('#check',{sitekey:params.get('key'),language:params.get('lang')||'auto',action:'off42_assistant',
      callback:report,'error-callback':()=>report(''),'expired-callback':()=>report('')});
  } catch { report(''); }
};
const script=document.createElement('script');
script.src='https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit&onload=off42Ready';
script.onerror=()=>report('');
document.head.append(script);
