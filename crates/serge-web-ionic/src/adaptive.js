const panels = ".serge-ionic-panel";
const focusable = 'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])';

export function mountAdaptive(root, css) {
    const doc = root.ownerDocument;
    const win = doc.defaultView;
    if (!doc.getElementById("serge-ionic-css")) {
        const style = doc.createElement("style");
        style.id = "serge-ionic-css";
        style.textContent = css;
        doc.head.append(style);
    }
    const events = new win.AbortController();
    const options = {signal: events.signal};
    const returns = new Map();
    let active = null;
    let gesture = null;
    function topPanel() {
        const all = doc.querySelectorAll(panels);
        const top = all[all.length - 1];
        return root.contains(top || null) ? top : null;
    }
    function controls(panel) {
        return Array.from(panel.querySelectorAll(focusable)).filter(el => el.getClientRects().length && !el.closest("[inert],[hidden]"));
    }
    function focusPanel(panel) {
        (controls(panel)[0] || panel).focus({preventScroll: true});
    }
    function sync() {
        const previous = active;
        active = topPanel();
        if (active === previous) return;
        if (previous && !previous.isConnected) {
            const trigger = returns.get(previous);
            returns.delete(previous);
            if (trigger?.isConnected && (!active || active.contains(trigger))) trigger.focus({preventScroll: true});
        }
        if (active && !returns.has(active)) returns.set(active, doc.activeElement);
        if (active && !active.contains(doc.activeElement)) focusPanel(active);
    }
    function close(panel) {
        panel?.querySelector(".serge-ionic-dismiss")?.click();
    }
    const observer = new win.MutationObserver(sync);
    observer.observe(root, {childList: true, subtree: true});
    sync();
    doc.addEventListener("keydown", e => {
        const panel = topPanel();
        if (!panel) return;
        if (e.key === "Escape") {
            e.preventDefault();
            close(panel);
        } else if (e.key === "Tab") {
            const items = controls(panel);
            const first = items[0] || panel;
            const last = items.at(-1) || panel;
            if (!panel.contains(doc.activeElement) || (e.shiftKey ? doc.activeElement === first : doc.activeElement === last)) {
                e.preventDefault();
                (e.shiftKey ? last : first).focus();
            }
        }
    }, options);
    doc.addEventListener("focusin", e => {
        const panel = topPanel();
        if (panel && !panel.contains(e.target)) focusPanel(panel);
    }, options);
    root.addEventListener("click", e => {
        if (e.target.matches?.(".serge-ionic-backdrop") && e.target.contains(topPanel())) close(topPanel());
    }, options);
    root.addEventListener("pointerdown", e => {
        const grip = e.target.closest?.(".serge-ionic-grip");
        if (!e.isPrimary || !grip || !topPanel()?.contains(grip)) return;
        gesture = {id: e.pointerId, x: e.clientX, y: e.clientY, panel: topPanel()};
        if (grip.hasPointerCapture && e.isTrusted) grip.setPointerCapture(e.pointerId);
    }, options);
    root.addEventListener("pointerup", e => {
        if (gesture?.id !== e.pointerId) return;
        const {x, y, panel} = gesture;
        gesture = null;
        if (panel === topPanel() && e.clientY-y > 56 && Math.abs(e.clientX-x) < (e.clientY-y)/2) close(panel);
    }, options);
    root.addEventListener("pointercancel", () => {gesture = null;}, options);
    function measure() {
        const v = win.visualViewport;
        // Pinch zoom changes visualViewport without a soft keyboard.
        const keyboard = v && v.scale === 1 ? Math.max(0, win.innerHeight - v.height - v.offsetTop) : 0;
        root.style.setProperty("--serge-keyboard-inset", keyboard + "px");
        root.style.setProperty("--serge-viewport-height", (v && v.scale === 1 ? v.height : win.innerHeight) + "px");
    }
    win.visualViewport?.addEventListener("resize", measure, options);
    win.visualViewport?.addEventListener("scroll", measure, options);
    win.addEventListener("resize", measure, options);
    measure();
    return () => {
        events.abort();
        observer.disconnect();
        const trigger = returns.get(active);
        if (trigger?.isConnected) trigger.focus({preventScroll: true});
        returns.clear();
        root.style.removeProperty("--serge-keyboard-inset");
        root.style.removeProperty("--serge-viewport-height");
    };
}
