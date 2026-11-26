__IMPORTS__
const page = document.querySelector("output");
sergeWebview.connect({
  buildId:"__BUILD__",
  __CODEC__
  onState(value) { page.textContent = String(value); },
  onStatus(status) {
    if (status === "ready" && page.textContent === "0") setTimeout(() => sergeWebview.submit({op:"save"}), 0);
  },
  onResult(id, outcome) {
    if (outcome.kind === "success" && outcome.value === "saved") sergeWebview.submit({op:"denied"});
    if (outcome.kind === "error") sergeWebview.submit({op:"done",rendered:page.textContent,guard:outcome.code,inline:!!window.inlineRan,width:window.innerWidth,height:window.innerHeight});
  }
});
