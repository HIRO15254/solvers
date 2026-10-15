/* @ds-bundle: {"format":4,"namespace":"Cryst","components":[{"name":"Button","sourcePath":"components/actions/Button.jsx"},{"name":"IconButton","sourcePath":"components/actions/IconButton.jsx"},{"name":"Icon","sourcePath":"components/core/Icon.jsx"},{"name":"Badge","sourcePath":"components/display/Badge.jsx"},{"name":"Avatar","sourcePath":"components/display/Avatar.jsx"},{"name":"Card","sourcePath":"components/display/Card.jsx"},{"name":"Tag","sourcePath":"components/display/Tag.jsx"},{"name":"Kbd","sourcePath":"components/display/Kbd.jsx"},{"name":"Alert","sourcePath":"components/display/Alert.jsx"},{"name":"EmptyState","sourcePath":"components/display/EmptyState.jsx"},{"name":"SelectionBar","sourcePath":"components/display/SelectionBar.jsx"},{"name":"FilterBar","sourcePath":"components/display/FilterBar.jsx"},{"name":"Separator","sourcePath":"components/display/Separator.jsx"},{"name":"Skeleton","sourcePath":"components/display/Skeleton.jsx"},{"name":"Progress","sourcePath":"components/display/Progress.jsx"},{"name":"Table","sourcePath":"components/display/Table.jsx"},{"name":"Checkbox","sourcePath":"components/forms/Checkbox.jsx"},{"name":"Input","sourcePath":"components/forms/Input.jsx"},{"name":"Textarea","sourcePath":"components/forms/Textarea.jsx"},{"name":"Radio","sourcePath":"components/forms/Radio.jsx"},{"name":"SegmentedControl","sourcePath":"components/forms/SegmentedControl.jsx"},{"name":"RadioCard","sourcePath":"components/forms/RadioCard.jsx"},{"name":"Select","sourcePath":"components/forms/Select.jsx"},{"name":"Switch","sourcePath":"components/forms/Switch.jsx"},{"name":"Tabs","sourcePath":"components/navigation/Tabs.jsx"},{"name":"Pagination","sourcePath":"components/navigation/Pagination.jsx"},{"name":"Sidebar","sourcePath":"components/navigation/Sidebar.jsx"},{"name":"List","sourcePath":"components/navigation/List.jsx"},{"name":"Popover","sourcePath":"components/overlay/Popover.jsx"},{"name":"DropdownMenu","sourcePath":"components/overlay/DropdownMenu.jsx"},{"name":"CommandPalette","sourcePath":"components/overlay/CommandPalette.jsx"},{"name":"BottomSheet","sourcePath":"components/overlay/BottomSheet.jsx"},{"name":"Dialog","sourcePath":"components/overlay/Dialog.jsx"},{"name":"Toast","sourcePath":"components/overlay/Toast.jsx"},{"name":"Tooltip","sourcePath":"components/overlay/Tooltip.jsx"},{"name":"ShortcutsDialog","sourcePath":"components/overlay/ShortcutsDialog.jsx"}]} */
(() => {

const __ds_ns = (window.Cryst = window.Cryst || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/core/utils.jsx
try { (() => {
function ensureCss(id, text) {
  if (typeof document !== "undefined" && !document.getElementById(id)) {
    const s = document.createElement("style");
    s.id = id;
    s.textContent = text;
    document.head.appendChild(s);
  }
}
function cx(...a) {
  return a.filter(Boolean).join(" ");
}
function sizeOf(size, fallback = "md") {
  return !size || size === "default" ? fallback : size;
}
function useControllable(value, defaultValue, onChange) {
  const [inner, setInner] = React.useState(defaultValue);
  const controlled = value !== void 0;
  const current = controlled ? value : inner;
  const set = React.useCallback((v) => {
    if (!controlled) setInner(v);
    if (onChange) onChange(v);
  }, [controlled, onChange]);
  return [current, set];
}
Object.assign(__ds_scope, { ensureCss, cx, sizeOf, useControllable });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/utils.jsx", error: String((e && e.message) || e) }); }

// components/core/status.jsx
try { (() => {
const STATUS = {
  neutral: { icon: "info-circle", color: "var(--muted-foreground)" },
  info: { icon: "info-circle", color: "var(--info)" },
  success: { icon: "circle-check", color: "var(--success)" },
  warning: { icon: "alert-triangle", color: "var(--warning)" },
  destructive: { icon: "circle-x", color: "var(--destructive)" }
};
function status(name) {
  return STATUS[name === "default" ? "neutral" : name] || STATUS.neutral;
}
function isUrgent(name) {
  return name === "warning" || name === "destructive";
}
Object.assign(__ds_scope, { STATUS, status, isUrgent });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/status.jsx", error: String((e && e.message) || e) }); }

// components/core/Icon.jsx
try { (() => {
const { cx } = __ds_scope;
function Icon({ name, size = 16, color, filled = false, className = "", style, ...props }) {
  return /* @__PURE__ */ React.createElement("i", { "aria-hidden": "true", "data-slot": "icon", ...props, className: cx("ti", `ti-${name}${filled ? "-filled" : ""}`, className), style: { fontSize: size, lineHeight: 1, color, display: "inline-block", ...style } });
}
Object.assign(__ds_scope, { Icon });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/core/Icon.jsx", error: String((e && e.message) || e) }); }

// components/actions/Button.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-btn{display:inline-flex;align-items:center;justify-content:center;gap:6px;font-family:var(--font-sans);font-weight:var(--weight-medium);letter-spacing:var(--tracking-body);border-radius:var(--radius-md);border:1px solid transparent;cursor:pointer;white-space:nowrap;text-decoration:none;transition:background var(--duration-fast) var(--ease-out),color var(--duration-fast) var(--ease-out),border-color var(--duration-fast) var(--ease-out),filter var(--duration-fast) var(--ease-out)}
.cr-btn:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-btn:disabled,.cr-btn[aria-disabled=true]{opacity:.5;cursor:not-allowed;pointer-events:none}
.cr-btn-sm{height:var(--control-sm);padding:0 10px;font-size:var(--text-xs)}
.cr-btn-md{height:var(--control-md);padding:0 12px;font-size:var(--text-sm)}
.cr-btn-lg{height:var(--control-lg);padding:0 16px;font-size:var(--text-sm)}
.cr-btn-xl{height:var(--m-control,44px);padding:0 18px;font-size:var(--m-text-body,15px);border-radius:var(--radius-lg)}
.cr-btn-icon{width:var(--control-md);height:var(--control-md);padding:0}
.cr-btn-primary{background:var(--primary);color:var(--primary-foreground)}
.cr-btn-primary:hover:not(:disabled){filter:brightness(1.1)}
.cr-btn-primary:active:not(:disabled){filter:brightness(.96)}
.cr-btn-secondary{background:var(--secondary);color:var(--secondary-foreground)}
.cr-btn-secondary:hover:not(:disabled){background:var(--accent);color:var(--accent-foreground)}
.cr-btn-outline{background:var(--card);color:var(--foreground);border-color:var(--input);box-shadow:var(--shadow-sm)}
.cr-btn-outline:hover:not(:disabled){background:var(--accent);color:var(--accent-foreground)}
.cr-btn-ghost{background:transparent;color:var(--muted-foreground)}
.cr-btn-ghost:hover:not(:disabled){background:var(--accent);color:var(--foreground)}
.cr-btn-destructive{background:var(--destructive);color:var(--destructive-foreground)}
.cr-btn-destructive:hover:not(:disabled){filter:brightness(1.08)}
.cr-btn-link{background:transparent;color:var(--primary);padding-left:0;padding-right:0;height:auto;text-underline-offset:4px}
.cr-btn-link:hover:not(:disabled){text-decoration:underline}
.cr-btn:active:not(:disabled){transform:translateY(0.5px)}`;
const VARIANT = { default: "primary", primary: "primary", secondary: "secondary", outline: "outline", ghost: "ghost", destructive: "destructive", link: "link" };
const SIZE = { default: "md", md: "md", sm: "sm", lg: "lg", xl: "xl", icon: "icon" };
function buttonVariants({ variant = "default", size = "default", className = "" } = {}) {
  ensureCss("cr-btn-css", css);
  return cx("cr-btn", `cr-btn-${SIZE[size] || "md"}`, `cr-btn-${VARIANT[variant] || "primary"}`, className);
}
const Button = React.forwardRef(function Button2({ variant = "default", size = "default", icon, iconRight, fullWidth = false, asChild = false, disabled = false, children, type = "button", style, className = "", ...props }, ref) {
  const cls = buttonVariants({ variant, size, className });
  const s = SIZE[size] || "md";
  const iconSize = s === "sm" ? 14 : s === "xl" ? 17 : 16;
  const st = { width: fullWidth ? "100%" : void 0, ...style };
  if (asChild && React.isValidElement(children)) {
    return React.cloneElement(children, { ...props, ref, "data-slot": "button", className: cx(cls, children.props.className), style: { ...st, ...children.props.style }, "aria-disabled": disabled || void 0 });
  }
  return /* @__PURE__ */ React.createElement("button", { ref, "data-slot": "button", type, disabled, className: cls, style: st, ...props }, icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: iconSize }), children, iconRight && /* @__PURE__ */ React.createElement(Icon, { name: iconRight, size: iconSize }));
});
Object.assign(__ds_scope, { buttonVariants, Button });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/actions/Button.jsx", error: String((e && e.message) || e) }); }

// components/actions/IconButton.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-iconbtn{display:inline-flex;align-items:center;justify-content:center;border-radius:var(--radius-md);border:1px solid transparent;cursor:pointer;background:transparent;color:var(--muted-foreground);padding:0;transition:background var(--duration-fast) var(--ease-out),color var(--duration-fast) var(--ease-out)}
.cr-iconbtn:hover:not(:disabled){background:var(--accent);color:var(--foreground)}
.cr-iconbtn:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-iconbtn:disabled{opacity:.5;cursor:not-allowed}
.cr-iconbtn-outline{background:var(--card);border-color:var(--input);box-shadow:var(--shadow-sm);color:var(--foreground)}
.cr-iconbtn-secondary{background:var(--secondary);color:var(--secondary-foreground)}
.cr-iconbtn-active{background:var(--accent);color:var(--foreground)}
.cr-iconbtn-sm{width:var(--control-sm);height:var(--control-sm)}
.cr-iconbtn-md{width:var(--control-md);height:var(--control-md)}
.cr-iconbtn-lg{width:var(--control-lg);height:var(--control-lg)}
.cr-iconbtn-xl{width:var(--m-control,44px);height:var(--m-control,44px)}`;
const IconButton = React.forwardRef(function IconButton2({ icon, label, variant = "ghost", size = "md", disabled = false, active = false, type = "button", style, className = "", ...props }, ref) {
  ensureCss("cr-iconbtn-css", css);
  const s = size === "icon" ? "md" : sizeOf(size);
  return /* @__PURE__ */ React.createElement("button", { ref, "data-slot": "icon-button", type, "aria-label": label, title: label, "aria-pressed": active || void 0, disabled, className: cx("cr-iconbtn", `cr-iconbtn-${s}`, (variant === "outline" || variant === "secondary") && `cr-iconbtn-${variant}`, active && "cr-iconbtn-active", className), style, ...props }, /* @__PURE__ */ React.createElement(Icon, { name: icon, size: s === "sm" ? 14 : s === "xl" ? 18 : 16 }));
});
Object.assign(__ds_scope, { IconButton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/actions/IconButton.jsx", error: String((e && e.message) || e) }); }

// components/display/Badge.jsx
try { (() => {
const { Icon, ensureCss, cx, STATUS } = __ds_scope;
const styles = {
  default: ["var(--primary)", "var(--primary-foreground)"],
  secondary: ["var(--secondary)", "var(--secondary-foreground)"],
  primary: ["var(--selection)", "var(--primary)"]
};
const tint = (c) => `color-mix(in oklab, var(--${c}) 15%, transparent)`;
const tints = { neutral: "var(--muted)", info: tint("info"), success: tint("success"), warning: tint("warning"), destructive: tint("destructive") };
const colors = Object.fromEntries(Object.entries(tints).map(([k, bg]) => [k, [bg, STATUS[k].color]]).concat(Object.entries(styles)));
const css = `.cr-badge{display:inline-flex;align-items:center;gap:4px;height:20px;padding:0 7px;border-radius:var(--radius-full);border:1px solid transparent;font-family:var(--font-sans);font-size:var(--text-xs);font-weight:var(--weight-medium);white-space:nowrap}
.cr-badge-outline{background:transparent!important;border-color:var(--border);color:var(--muted-foreground)!important}`;
function Badge({ variant = "neutral", icon, dot = false, outline = false, asChild = false, children, style, className = "", ...props }) {
  ensureCss("cr-badge-css", css);
  const isOutline = outline || variant === "outline";
  const [bg, fg] = colors[variant] || colors.neutral;
  const cls = cx("cr-badge", isOutline && "cr-badge-outline", className);
  const st = { background: bg, color: fg, ...style };
  if (asChild && React.isValidElement(children)) return React.cloneElement(children, { ...props, "data-slot": "badge", className: cx(cls, children.props.className), style: { ...st, ...children.props.style } });
  return /* @__PURE__ */ React.createElement("span", { "data-slot": "badge", className: cls, style: st, ...props }, dot && /* @__PURE__ */ React.createElement("span", { style: { width: 6, height: 6, borderRadius: "50%", background: "currentColor" } }), icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 12 }), children);
}
Object.assign(__ds_scope, { Badge });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Badge.jsx", error: String((e && e.message) || e) }); }

// components/display/Avatar.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-avatar{position:relative;display:inline-flex;flex:none;overflow:hidden;border-radius:var(--radius-full);background:var(--muted);font-family:var(--font-sans)}
.cr-avatar-img{width:100%;height:100%;object-fit:cover}
.cr-avatar-fallback{display:flex;width:100%;height:100%;align-items:center;justify-content:center;background:var(--selection);color:var(--primary);font-weight:var(--weight-semibold);letter-spacing:0}`;
const Ctx = React.createContext(null);
function Avatar({ src, alt = "", fallback, size = 32, children, style, className = "", ...props }) {
  ensureCss("cr-avatar-css", css);
  const [status, setStatus] = React.useState("idle");
  return /* @__PURE__ */ React.createElement(Ctx.Provider, { value: { status, setStatus, size } }, /* @__PURE__ */ React.createElement("span", { "data-slot": "avatar", className: cx("cr-avatar", className), style: { width: size, height: size, ...style }, ...props }, children || /* @__PURE__ */ React.createElement(React.Fragment, null, src && /* @__PURE__ */ React.createElement(AvatarImage, { src, alt }), /* @__PURE__ */ React.createElement(AvatarFallback, null, fallback))));
}
function AvatarImage({ src, alt = "", className = "", ...props }) {
  const c = React.useContext(Ctx);
  React.useEffect(() => {
    if (!src) {
      c && c.setStatus("error");
      return;
    }
    c && c.setStatus("loading");
    const i = new Image();
    i.onload = () => c && c.setStatus("loaded");
    i.onerror = () => c && c.setStatus("error");
    i.src = src;
  }, [src]);
  if (!c || c.status !== "loaded") return null;
  return /* @__PURE__ */ React.createElement("img", { "data-slot": "avatar-image", src, alt, className: cx("cr-avatar-img", className), ...props });
}
function AvatarFallback({ children, className = "", style, ...props }) {
  const c = React.useContext(Ctx);
  if (c && c.status === "loaded") return null;
  const size = c ? c.size : 32;
  return /* @__PURE__ */ React.createElement("span", { "data-slot": "avatar-fallback", className: cx("cr-avatar-fallback", className), style: { fontSize: Math.max(9, Math.round(size * 0.4)), ...style }, ...props }, children);
}
Object.assign(__ds_scope, { Avatar, AvatarImage, AvatarFallback });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Avatar.jsx", error: String((e && e.message) || e) }); }

// components/display/Card.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-card{display:flex;flex-direction:column;background:var(--card);border:1px solid var(--border);border-radius:var(--radius-lg);box-shadow:var(--shadow-sm);font-family:var(--font-sans);color:var(--card-foreground)}
.cr-card-head{display:flex;align-items:flex-start;gap:10px;padding:16px 16px 0}
.cr-card-head-text{flex:1;min-width:0}
.cr-card-title{font-size:var(--text-base);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-heading);line-height:var(--leading-tight);margin:0}
.cr-card-desc{font-size:var(--text-sm);color:var(--muted-foreground);margin:2px 0 0}
.cr-card-action{margin-left:auto;align-self:flex-start}
.cr-card-body{padding:16px;font-size:var(--text-sm)}
.cr-card-foot{padding:12px 16px;border-top:1px solid var(--border);display:flex;align-items:center;justify-content:flex-end;gap:8px}`;
function Card({ title, description, icon, footer, children, style, className = "", ...props }) {
  ensureCss("cr-card-css", css);
  const short = title != null || description != null || icon != null || footer != null;
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "card", className: cx("cr-card", className), style, ...props }, short ? /* @__PURE__ */ React.createElement(React.Fragment, null, (title || description) && /* @__PURE__ */ React.createElement("div", { className: "cr-card-head" }, icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 18, style: { marginTop: 2, color: "var(--muted-foreground)" } }), /* @__PURE__ */ React.createElement("div", { className: "cr-card-head-text" }, title && /* @__PURE__ */ React.createElement("h3", { className: "cr-card-title" }, title), description && /* @__PURE__ */ React.createElement("p", { className: "cr-card-desc" }, description))), children != null && /* @__PURE__ */ React.createElement("div", { className: "cr-card-body" }, children), footer && /* @__PURE__ */ React.createElement("div", { className: "cr-card-foot" }, footer)) : children);
}
function CardHeader({ className = "", children, ...p }) {
  ensureCss("cr-card-css", css);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "card-header", className: cx("cr-card-head", className), ...p }, /* @__PURE__ */ React.createElement("div", { className: "cr-card-head-text" }, React.Children.toArray(children).filter((c) => !(c && c.type === CardAction))), React.Children.toArray(children).filter((c) => c && c.type === CardAction));
}
function CardTitle({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("h3", { "data-slot": "card-title", className: cx("cr-card-title", className), ...p });
}
function CardDescription({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("p", { "data-slot": "card-description", className: cx("cr-card-desc", className), ...p });
}
function CardAction({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "card-action", className: cx("cr-card-action", className), ...p });
}
function CardContent({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "card-content", className: cx("cr-card-body", className), ...p });
}
function CardFooter({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "card-footer", className: cx("cr-card-foot", className), ...p });
}
Object.assign(__ds_scope, { Card, CardHeader, CardTitle, CardDescription, CardAction, CardContent, CardFooter });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Card.jsx", error: String((e && e.message) || e) }); }

// components/display/Tag.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-tag{display:inline-flex;align-items:center;gap:5px;height:24px;padding:0 8px;border-radius:var(--radius-md);border:1px solid var(--border);background:var(--card);font-family:var(--font-sans);font-size:var(--text-xs);font-weight:var(--weight-medium);color:var(--foreground);white-space:nowrap}
.cr-tag-x{display:inline-flex;border:none;background:none;padding:0;margin-left:1px;color:var(--muted-foreground);cursor:pointer;border-radius:2px}
.cr-tag-x:hover{color:var(--foreground)}
.cr-tag-x:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}`;
function Tag({ icon, color, onRemove, removeLabel = "Remove", children, style, className = "", ...props }) {
  ensureCss("cr-tag-css", css);
  return /* @__PURE__ */ React.createElement("span", { "data-slot": "tag", className: cx("cr-tag", className), style, ...props }, color && /* @__PURE__ */ React.createElement("span", { style: { width: 8, height: 8, borderRadius: "50%", background: color } }), icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 12, color: "var(--muted-foreground)" }), children, onRemove && /* @__PURE__ */ React.createElement("button", { type: "button", "aria-label": removeLabel, className: "cr-tag-x", onClick: onRemove }, /* @__PURE__ */ React.createElement(Icon, { name: "x", size: 12 })));
}
Object.assign(__ds_scope, { Tag });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Tag.jsx", error: String((e && e.message) || e) }); }

// components/display/Kbd.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-kbd{display:inline-flex;align-items:center;gap:3px;font-family:var(--font-mono);font-size:var(--text-xs);color:var(--muted-foreground);white-space:nowrap}
.cr-kbd-key{display:inline-flex;align-items:center;justify-content:center;box-sizing:border-box;min-width:20px;height:20px;padding:0 4px;border:1px solid var(--border);border-bottom-width:2px;border-radius:var(--radius-sm);background:var(--muted);color:var(--foreground);font-size:11px;line-height:1}
.cr-kbd-plain .cr-kbd-key{border:none;background:none;padding:0;min-width:0;color:inherit;height:auto}`;
function Kbd({ keys, plain = false, children, className = "", ...props }) {
  ensureCss("cr-kbd-css", css);
  const list = keys || (typeof children === "string" ? [children] : null);
  return /* @__PURE__ */ React.createElement("kbd", { "data-slot": "kbd", className: cx("cr-kbd", plain && "cr-kbd-plain", className), ...props }, list ? list.map((k, i) => /* @__PURE__ */ React.createElement("span", { key: i, className: "cr-kbd-key" }, k)) : children);
}
Object.assign(__ds_scope, { Kbd });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Kbd.jsx", error: String((e && e.message) || e) }); }

// components/display/Alert.jsx
try { (() => {
const { Icon, ensureCss, cx, status, isUrgent } = __ds_scope;
const css = `.cr-alert{display:grid;box-shadow:none;grid-template-columns:auto 1fr;gap:10px;padding:12px 14px;border:1px solid var(--border);border-radius:var(--radius-lg);background:var(--card);color:var(--card-foreground);font-family:var(--font-sans);font-size:var(--text-sm)}
.cr-alert-noicon{grid-template-columns:1fr}
.cr-alert-title{font-size:var(--text-sm);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-heading);margin:0}
.cr-alert-desc{font-size:var(--text-sm);color:var(--muted-foreground);margin:2px 0 0}
.cr-alert-actions{display:flex;gap:8px;margin-top:10px}`;
function Alert({ variant = "neutral", icon, title, description, actions, children, className = "", style, ...props }) {
  ensureCss("cr-alert-css", css);
  const meta = status(variant);
  const glyph = icon === null ? null : icon || meta.icon;
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "alert", role: isUrgent(variant) ? "alert" : "status", className: cx("cr-alert", !glyph && "cr-alert-noicon", className), style, ...props }, glyph && /* @__PURE__ */ React.createElement(Icon, { name: glyph, size: 16, color: meta.color, style: { marginTop: 1 } }), /* @__PURE__ */ React.createElement("div", null, title && /* @__PURE__ */ React.createElement(AlertTitle, null, title), description && /* @__PURE__ */ React.createElement(AlertDescription, null, description), children, actions && /* @__PURE__ */ React.createElement("div", { className: "cr-alert-actions" }, actions)));
}
function AlertTitle({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("p", { "data-slot": "alert-title", className: cx("cr-alert-title", className), ...p });
}
function AlertDescription({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("p", { "data-slot": "alert-description", className: cx("cr-alert-desc", className), ...p });
}
Object.assign(__ds_scope, { Alert, AlertTitle, AlertDescription });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Alert.jsx", error: String((e && e.message) || e) }); }

// components/display/EmptyState.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-empty{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:6px;padding:48px 24px;text-align:center;font-family:var(--font-sans);color:var(--foreground)}
.cr-empty-sm{padding:24px 16px}
.cr-empty-icon{color:var(--muted-foreground);margin-bottom:2px}
.cr-empty-title{font-size:var(--text-sm);font-weight:var(--weight-medium);margin:0}
.cr-empty-desc{font-size:var(--text-sm);color:var(--muted-foreground);margin:0;max-width:44ch}
.cr-empty-action{margin-top:10px;display:flex;gap:8px}`;
function EmptyState({ icon = "inbox", title, description, action, size = "md", className = "", style, ...props }) {
  ensureCss("cr-empty-css", css);
  const s = sizeOf(size);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "empty-state", className: cx("cr-empty", s === "sm" && "cr-empty-sm", className), style, ...props }, icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: s === "sm" ? 20 : 24, className: "cr-empty-icon" }), title && /* @__PURE__ */ React.createElement("p", { className: "cr-empty-title" }, title), description && /* @__PURE__ */ React.createElement("p", { className: "cr-empty-desc" }, description), action && /* @__PURE__ */ React.createElement("div", { className: "cr-empty-action" }, action));
}
Object.assign(__ds_scope, { EmptyState });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/EmptyState.jsx", error: String((e && e.message) || e) }); }

// components/display/SelectionBar.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-selbar{display:flex;align-items:center;gap:10px;min-height:var(--control-lg);padding:0 8px 0 12px;border:1px solid var(--border);border-radius:var(--radius-md);background:var(--card);box-shadow:var(--shadow-sm);font-family:var(--font-sans);font-size:var(--text-sm)}
.cr-selbar-count{font-weight:var(--weight-medium);white-space:nowrap}
.cr-selbar-actions{display:flex;align-items:center;gap:6px;margin-left:auto}
.cr-selbar-clear{display:inline-flex;align-items:center;gap:4px;border:none;background:none;padding:4px 6px;border-radius:var(--radius-sm);font:inherit;font-size:var(--text-xs);color:var(--muted-foreground);cursor:pointer}
.cr-selbar-clear:hover{background:var(--accent);color:var(--foreground)}
.cr-selbar-clear:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}`;
function SelectionBar({ count = 0, label, onClear, clearLabel = "Clear selection", actions, hideWhenEmpty = true, className = "", style, ...props }) {
  ensureCss("cr-selbar-css", css);
  if (hideWhenEmpty && !count) return null;
  return /* @__PURE__ */ React.createElement("div", { role: "region", "aria-live": "polite", "data-slot": "selection-bar", className: cx("cr-selbar", className), style, ...props }, /* @__PURE__ */ React.createElement("span", { className: "cr-selbar-count" }, label || `${count} selected`), onClear && /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-selbar-clear", onClick: onClear }, /* @__PURE__ */ React.createElement(Icon, { name: "x", size: 12 }), clearLabel), /* @__PURE__ */ React.createElement("div", { className: "cr-selbar-actions" }, actions));
}
Object.assign(__ds_scope, { SelectionBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/SelectionBar.jsx", error: String((e && e.message) || e) }); }

// components/display/FilterBar.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-filters{display:flex;align-items:center;gap:6px;flex-wrap:wrap;font-family:var(--font-sans)}
.cr-filter{display:inline-flex;align-items:center;gap:6px;height:26px;padding:0 4px 0 8px;border:1px solid var(--input);border-radius:var(--radius-md);background:var(--card);font-size:var(--text-xs);color:var(--foreground);white-space:nowrap}
.cr-filter-field{color:var(--muted-foreground)}
.cr-filter-value{font-weight:var(--weight-medium)}
.cr-filter-x{display:inline-flex;align-items:center;justify-content:center;width:18px;height:18px;border:none;background:none;border-radius:var(--radius-sm);color:var(--muted-foreground);cursor:pointer}
.cr-filter-x:hover{background:var(--accent);color:var(--foreground)}
.cr-filter-x:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}
.cr-filters-clear{border:none;background:none;padding:4px 6px;border-radius:var(--radius-sm);font:inherit;font-size:var(--text-xs);color:var(--muted-foreground);cursor:pointer}
.cr-filters-clear:hover{background:var(--accent);color:var(--foreground)}
.cr-filters-clear:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}`;
function FilterBar({ filters = [], onRemove, onClear, clearLabel = "Clear all", removeLabel = "Remove filter", children, className = "", style, ...props }) {
  ensureCss("cr-filters-css", css);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "filter-bar", className: cx("cr-filters", className), style, ...props }, filters.map((f) => {
    var _a;
    return /* @__PURE__ */ React.createElement("span", { key: (_a = f.id) != null ? _a : f.label, className: "cr-filter" }, f.icon && /* @__PURE__ */ React.createElement(Icon, { name: f.icon, size: 12, color: "var(--muted-foreground)" }), f.field && /* @__PURE__ */ React.createElement("span", { className: "cr-filter-field" }, f.field), /* @__PURE__ */ React.createElement("span", { className: "cr-filter-value" }, f.label), onRemove && /* @__PURE__ */ React.createElement("button", { type: "button", "aria-label": `${removeLabel}: ${f.field ? f.field + " " : ""}${f.label}`, className: "cr-filter-x", onClick: () => {
      var _a2;
      return onRemove((_a2 = f.id) != null ? _a2 : f.label);
    } }, /* @__PURE__ */ React.createElement(Icon, { name: "x", size: 11 })));
  }), children, onClear && filters.length > 1 && /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-filters-clear", onClick: onClear }, clearLabel));
}
Object.assign(__ds_scope, { FilterBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/FilterBar.jsx", error: String((e && e.message) || e) }); }

// components/display/Separator.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-sep{flex:none;background:var(--border)}
.cr-sep-h{height:1px;width:100%;margin:0}
.cr-sep-v{width:1px;align-self:stretch;min-height:1em}`;
function Separator({ orientation = "horizontal", decorative = true, className = "", style, ...props }) {
  ensureCss("cr-sep-css", css);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "separator", role: decorative ? "none" : "separator", "aria-orientation": decorative ? void 0 : orientation, className: cx("cr-sep", orientation === "vertical" ? "cr-sep-v" : "cr-sep-h", className), style, ...props });
}
Object.assign(__ds_scope, { Separator });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Separator.jsx", error: String((e && e.message) || e) }); }

// components/display/Skeleton.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-skel{display:block;background:var(--muted);border-radius:var(--radius-md);animation:cr-skel-pulse 1.6s var(--ease-in-out) infinite}
.cr-skel-circle{border-radius:var(--radius-full)}
.cr-skel-text{height:1em;border-radius:var(--radius-sm)}
@keyframes cr-skel-pulse{0%,100%{opacity:1}50%{opacity:.55}}
@media (prefers-reduced-motion:reduce){.cr-skel{animation:none}}`;
function Skeleton({ width, height, circle = false, text = false, className = "", style, ...props }) {
  ensureCss("cr-skel-css", css);
  return /* @__PURE__ */ React.createElement("span", { "data-slot": "skeleton", "aria-hidden": "true", className: cx("cr-skel", circle && "cr-skel-circle", text && "cr-skel-text", className), style: { width, height, ...style }, ...props });
}
Object.assign(__ds_scope, { Skeleton });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Skeleton.jsx", error: String((e && e.message) || e) }); }

// components/display/Progress.jsx
try { (() => {
const { ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-progress{display:block;width:100%;height:6px;border-radius:var(--radius-full);background:var(--muted);overflow:hidden}
.cr-progress-sm{height:4px}
.cr-progress-lg{height:10px}
.cr-progress-bar{height:100%;border-radius:inherit;background:var(--primary);transition:width var(--duration-base) var(--ease-out)}
.cr-progress-success .cr-progress-bar{background:var(--success)}
.cr-progress-warning .cr-progress-bar{background:var(--warning)}
.cr-progress-destructive .cr-progress-bar{background:var(--destructive)}
.cr-progress-indeterminate .cr-progress-bar{width:40%;animation:cr-progress-slide 1.2s var(--ease-in-out) infinite}
@keyframes cr-progress-slide{from{transform:translateX(-100%)}to{transform:translateX(250%)}}
@media (prefers-reduced-motion:reduce){.cr-progress-indeterminate .cr-progress-bar{animation:none;width:100%}}`;
function Progress({ value = 0, max = 100, variant = "default", size = "md", label, className = "", style, ...props }) {
  ensureCss("cr-progress-css", css);
  const indeterminate = value === null || value === void 0;
  const pct = indeterminate ? 0 : Math.max(0, Math.min(100, value / max * 100));
  const s = sizeOf(size);
  const cls = cx("cr-progress", s !== "md" && `cr-progress-${s}`, variant !== "default" && `cr-progress-${variant}`, indeterminate && "cr-progress-indeterminate", className);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "progress", role: "progressbar", "aria-label": label, "aria-valuemin": 0, "aria-valuemax": max, "aria-valuenow": indeterminate ? void 0 : value, className: cls, style, ...props }, /* @__PURE__ */ React.createElement("div", { className: "cr-progress-bar", style: indeterminate ? void 0 : { width: pct + "%" } }));
}
Object.assign(__ds_scope, { Progress });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Progress.jsx", error: String((e && e.message) || e) }); }

// components/display/Table.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-tablewrap{width:100%;overflow-x:auto}
.cr-table{width:100%;border-collapse:collapse;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground)}
.cr-table caption{caption-side:bottom;padding-top:10px;font-size:var(--text-xs);color:var(--muted-foreground);text-align:left}
.cr-th{height:32px;padding:0 12px;text-align:left;font-size:var(--text-xs);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted-foreground);background:var(--muted);border-bottom:1px solid var(--border);white-space:nowrap}
.cr-th-num{text-align:right}
.cr-th-sort{display:inline-flex;align-items:center;gap:4px;border:none;background:none;padding:0;margin:0;font:inherit;letter-spacing:inherit;text-transform:inherit;color:inherit;cursor:pointer}
.cr-th-sort:hover{color:var(--foreground)}
.cr-th-sort:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}
.cr-tr{border-bottom:1px solid var(--border)}
.cr-tbody .cr-tr:hover{background:var(--accent)}
.cr-tr-selected{background:var(--accent)}
.cr-td{height:38px;padding:0 12px;vertical-align:middle}
.cr-td-num{text-align:right;font-family:var(--font-mono);font-variant-numeric:tabular-nums}
.cr-tfoot{font-weight:var(--weight-medium);background:var(--muted)}
.cr-tfoot .cr-td{border-top:1px solid var(--border)}
.cr-table-compact .cr-td{height:32px}`;
function Table({ compact = false, children, className = "", style, ...props }) {
  ensureCss("cr-table-css", css);
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "table-container", className: "cr-tablewrap", style }, /* @__PURE__ */ React.createElement("table", { "data-slot": "table", className: cx("cr-table", compact && "cr-table-compact", className), ...props }, children));
}
function TableHeader({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("thead", { "data-slot": "table-header", className, ...p });
}
function TableBody({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("tbody", { "data-slot": "table-body", className: cx("cr-tbody", className), ...p });
}
function TableFooter({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("tfoot", { "data-slot": "table-footer", className: cx("cr-tfoot", className), ...p });
}
function TableRow({ selected = false, className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("tr", { "data-slot": "table-row", "data-state": selected ? "selected" : void 0, className: cx("cr-tr", selected && "cr-tr-selected", className), ...p });
}
function TableCaption({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("caption", { "data-slot": "table-caption", className, ...p });
}
function TableHead({ numeric = false, onSort, sorted = false, children, className = "", ...props }) {
  const label = /* @__PURE__ */ React.createElement(React.Fragment, null, children, onSort && /* @__PURE__ */ React.createElement(Icon, { name: sorted === "asc" ? "chevron-up" : sorted === "desc" ? "chevron-down" : "selector", size: 12, style: { opacity: sorted ? 1 : 0.5 } }));
  return /* @__PURE__ */ React.createElement("th", { "data-slot": "table-head", scope: "col", "aria-sort": onSort ? sorted === "asc" ? "ascending" : sorted === "desc" ? "descending" : "none" : void 0, className: cx("cr-th", numeric && "cr-th-num", className), ...props }, onSort ? /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-th-sort", onClick: onSort }, label) : label);
}
function TableCell({ numeric = false, className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("td", { "data-slot": "table-cell", className: cx("cr-td", numeric && "cr-td-num", className), ...p });
}
Object.assign(__ds_scope, { Table, TableHeader, TableBody, TableFooter, TableRow, TableCaption, TableHead, TableCell });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/display/Table.jsx", error: String((e && e.message) || e) }); }

// components/forms/Label.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-label{font-family:var(--font-sans);font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--foreground);line-height:var(--leading-tight);user-select:none}`;
const Label = React.forwardRef(function Label2({ className = "", ...props }, ref) {
  ensureCss("cr-label-css", css);
  return /* @__PURE__ */ React.createElement("label", { ref, "data-slot": "label", className: cx("cr-label", className), ...props });
});
Object.assign(__ds_scope, { Label });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Label.jsx", error: String((e && e.message) || e) }); }

// components/forms/Checkbox.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-check{position:relative;display:inline-flex;align-items:center;gap:8px;cursor:pointer;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground);user-select:none}
.cr-check-xl{min-height:var(--m-control,44px);gap:10px;font-size:var(--m-text-body,15px)}
.cr-check-xl .cr-checkbox{width:20px;height:20px;border-radius:var(--radius-md)}
.cr-check input{position:absolute;opacity:0;width:0;height:0}
.cr-checkbox{width:16px;height:16px;border-radius:var(--radius-sm);border:1px solid var(--input);background:var(--card);display:inline-flex;align-items:center;justify-content:center;color:transparent;transition:background var(--duration-fast) var(--ease-out),border-color var(--duration-fast) var(--ease-out);flex:none}
.cr-check input:checked+.cr-checkbox,.cr-checkbox[data-state=indeterminate]{background:var(--primary);border-color:var(--primary);color:var(--primary-foreground)}
.cr-check input:focus-visible+.cr-checkbox{box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-check input[aria-invalid=true]+.cr-checkbox{border-color:var(--destructive)}
.cr-check-disabled{opacity:.5;cursor:not-allowed}`;
const Checkbox = React.forwardRef(function Checkbox2({ label, checked, defaultChecked = false, onCheckedChange, onChange, disabled = false, size = "md", id, name, value, required, style, className = "", ...props }, ref) {
  ensureCss("cr-checkbox-css", css);
  const [state, setState] = useControllable(checked, defaultChecked, onCheckedChange);
  const indeterminate = state === "indeterminate";
  const inner = React.useRef(null);
  React.useImperativeHandle(ref, () => inner.current);
  React.useEffect(() => {
    if (inner.current) inner.current.indeterminate = indeterminate;
  }, [indeterminate]);
  return /* @__PURE__ */ React.createElement("label", { "data-slot": "checkbox", className: cx("cr-check", sizeOf(size) === "xl" && "cr-check-xl", disabled && "cr-check-disabled", className), style }, /* @__PURE__ */ React.createElement("input", { ref: inner, type: "checkbox", id, name, value, required, checked: state === true, onChange: (e) => {
    setState(e.target.checked);
    onChange && onChange(e);
  }, disabled, ...props }), /* @__PURE__ */ React.createElement("span", { className: "cr-checkbox", "data-state": indeterminate ? "indeterminate" : state ? "checked" : "unchecked" }, /* @__PURE__ */ React.createElement(Icon, { name: indeterminate ? "minus" : "check", size: sizeOf(size) === "xl" ? 14 : 12 })), label && /* @__PURE__ */ React.createElement("span", null, label));
});
Object.assign(__ds_scope, { Checkbox });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Checkbox.jsx", error: String((e && e.message) || e) }); }

// components/forms/Input.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-field{display:flex;flex-direction:column;gap:6px;font-family:var(--font-sans)}
.cr-field-label{font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--foreground)}
.cr-hint{font-size:var(--text-xs);color:var(--muted-foreground)}
.cr-hint-error{color:var(--destructive)}
.cr-inputwrap{position:relative;display:flex;align-items:center}
.cr-input{width:100%;box-sizing:border-box;height:var(--control-md);padding:0 10px;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground);background:var(--card);border:1px solid var(--input);border-radius:var(--radius-md);transition:border-color var(--duration-fast) var(--ease-out),box-shadow var(--duration-fast) var(--ease-out)}
.cr-input-sm{height:var(--control-sm);font-size:var(--text-xs);padding:0 8px}
.cr-input-lg{height:var(--control-lg)}
.cr-input-xl{height:var(--m-control,44px);font-size:var(--m-text-body,15px);border-radius:var(--radius-lg)}
.cr-input::placeholder{color:var(--muted-foreground)}
.cr-input:focus{outline:none;border-color:var(--ring);box-shadow:0 0 0 3px var(--selection)}
.cr-input:disabled{opacity:.5;cursor:not-allowed}
.cr-input[aria-invalid=true]{border-color:var(--destructive)}
.cr-input[aria-invalid=true]:focus{border-color:var(--destructive);box-shadow:0 0 0 3px color-mix(in oklab, var(--destructive) 20%, transparent)}
.cr-input-withicon{padding-left:30px}
.cr-inputicon{position:absolute;left:9px;color:var(--muted-foreground);pointer-events:none;display:flex}
.cr-input[type=file]{padding-top:5px}`;
const Input = React.forwardRef(function Input2({ label, hint, error, icon, size = "md", type = "text", id, style, className = "", "aria-invalid": ariaInvalid, ...props }, ref) {
  ensureCss("cr-input-css", css);
  const auto = React.useId ? React.useId() : void 0;
  const fid = id || (label || hint || error ? auto : void 0);
  const invalid = ariaInvalid != null ? ariaInvalid : error ? true : void 0;
  const s = sizeOf(size);
  const input = /* @__PURE__ */ React.createElement("div", { className: "cr-inputwrap" }, icon && /* @__PURE__ */ React.createElement("span", { className: "cr-inputicon" }, /* @__PURE__ */ React.createElement(Icon, { name: icon, size: s === "sm" ? 13 : s === "xl" ? 16 : 15 })), /* @__PURE__ */ React.createElement("input", { ref, "data-slot": "input", id: fid, type, "aria-invalid": invalid, "aria-describedby": (error || hint) && fid ? fid + "-hint" : void 0, className: cx("cr-input", s !== "md" && "cr-input-" + s, icon && "cr-input-withicon", className), style, ...props }));
  if (!label && !hint && !error) return input;
  return /* @__PURE__ */ React.createElement("div", { className: "cr-field" }, label && /* @__PURE__ */ React.createElement("label", { className: "cr-field-label", htmlFor: fid }, label), input, (error || hint) && /* @__PURE__ */ React.createElement("span", { id: fid + "-hint", className: cx("cr-hint", error && "cr-hint-error") }, error || hint));
});
Object.assign(__ds_scope, { Input });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Input.jsx", error: String((e && e.message) || e) }); }

// components/forms/Textarea.jsx
try { (() => {
const { ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-textarea{width:100%;box-sizing:border-box;min-height:72px;padding:8px 10px;font-family:var(--font-sans);font-size:var(--text-sm);line-height:var(--leading-normal);color:var(--foreground);background:var(--card);border:1px solid var(--input);border-radius:var(--radius-md);resize:vertical;transition:border-color var(--duration-fast) var(--ease-out),box-shadow var(--duration-fast) var(--ease-out)}
.cr-textarea::placeholder{color:var(--muted-foreground)}
.cr-textarea:focus{outline:none;border-color:var(--ring);box-shadow:0 0 0 3px var(--selection)}
.cr-textarea:disabled{opacity:.5;cursor:not-allowed}
.cr-textarea[aria-invalid=true]{border-color:var(--destructive)}
.cr-textarea-xl{font-size:var(--m-text-body,15px);border-radius:var(--radius-lg);padding:10px 12px}
.cr-textarea-count{display:flex;justify-content:flex-end;font-size:var(--text-xs);color:var(--muted-foreground)}
.cr-field{display:flex;flex-direction:column;gap:6px;font-family:var(--font-sans)}
.cr-field-label{font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--foreground)}
.cr-hint{font-size:var(--text-xs);color:var(--muted-foreground)}
.cr-hint-error{color:var(--destructive)}`;
const Textarea = React.forwardRef(function Textarea2({ label, hint, error, size = "md", rows = 3, maxLength, showCount = false, value, id, className = "", style, "aria-invalid": ariaInvalid, ...props }, ref) {
  ensureCss("cr-textarea-css", css);
  const auto = React.useId ? React.useId() : void 0;
  const fid = id || (label || hint || error ? auto : void 0);
  const field = /* @__PURE__ */ React.createElement("textarea", { ref, "data-slot": "textarea", id: fid, rows, maxLength, value, "aria-invalid": ariaInvalid != null ? ariaInvalid : error ? true : void 0, "aria-describedby": (error || hint) && fid ? fid + "-hint" : void 0, className: cx("cr-textarea", sizeOf(size) === "xl" && "cr-textarea-xl", className), style, ...props });
  if (!label && !hint && !error && !showCount) return field;
  return /* @__PURE__ */ React.createElement("div", { className: "cr-field" }, label && /* @__PURE__ */ React.createElement("label", { className: "cr-field-label", htmlFor: fid }, label), field, (error || hint) && /* @__PURE__ */ React.createElement("span", { id: fid + "-hint", className: cx("cr-hint", error && "cr-hint-error") }, error || hint), showCount && maxLength && /* @__PURE__ */ React.createElement("span", { className: "cr-textarea-count" }, String(value || "").length, " / ", maxLength));
});
Object.assign(__ds_scope, { Textarea });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Textarea.jsx", error: String((e && e.message) || e) }); }

// components/forms/Radio.jsx
try { (() => {
const { ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-radio-label{position:relative;display:inline-flex;align-items:center;gap:8px;cursor:pointer;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground);user-select:none}
.cr-radio-xl{min-height:var(--m-control,44px);gap:10px;font-size:var(--m-text-body,15px)}
.cr-radio-xl .cr-radio{width:20px;height:20px}
.cr-radio-xl .cr-radio::after{width:10px;height:10px}
.cr-radio-label input{position:absolute;opacity:0;width:0;height:0}
.cr-radio{width:16px;height:16px;border-radius:var(--radius-full);border:1px solid var(--input);background:var(--card);display:inline-flex;align-items:center;justify-content:center;transition:border-color var(--duration-fast) var(--ease-out);flex:none}
.cr-radio::after{content:'';width:8px;height:8px;border-radius:var(--radius-full);background:var(--primary);transform:scale(0);transition:transform var(--duration-fast) var(--ease-out)}
.cr-radio-label input:checked+.cr-radio{border-color:var(--primary)}
.cr-radio-label input:checked+.cr-radio::after{transform:scale(1)}
.cr-radio-label input:focus-visible+.cr-radio{box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-radio-disabled{opacity:.5;cursor:not-allowed}
.cr-radiogroup{display:flex;flex-direction:column;gap:8px}
.cr-radiogroup-horizontal{flex-direction:row;flex-wrap:wrap;gap:16px}`;
const GroupCtx = React.createContext(null);
const Radio = React.forwardRef(function Radio2({ label, name, value, checked, onChange, disabled = false, size, style, className = "", ...props }, ref) {
  ensureCss("cr-radio-css", css);
  const g = React.useContext(GroupCtx);
  const isChecked = g ? g.value === value : checked;
  const sz = sizeOf(size || g && g.size);
  const off = disabled || g && g.disabled;
  return /* @__PURE__ */ React.createElement("label", { "data-slot": "radio-group-item", className: cx("cr-radio-label", sz === "xl" && "cr-radio-xl", off && "cr-radio-disabled", className), style }, /* @__PURE__ */ React.createElement("input", { ref, type: "radio", name: g ? g.name : name, value, checked: isChecked, disabled: off, onChange: (e) => {
    if (g) g.setValue(value);
    onChange && onChange(e);
  }, ...props }), /* @__PURE__ */ React.createElement("span", { className: "cr-radio" }), label && /* @__PURE__ */ React.createElement("span", null, label));
});
function RadioGroup({ value, defaultValue, onValueChange, name, disabled = false, size, orientation = "vertical", children, style, className = "", ...props }) {
  ensureCss("cr-radio-css", css);
  const [v, setValue] = useControllable(value, defaultValue, onValueChange);
  const auto = React.useId ? React.useId() : "rg";
  return /* @__PURE__ */ React.createElement(GroupCtx.Provider, { value: { value: v, setValue, name: name || auto, disabled, size } }, /* @__PURE__ */ React.createElement("div", { role: "radiogroup", "data-slot": "radio-group", "aria-orientation": orientation, className: cx("cr-radiogroup", orientation === "horizontal" && "cr-radiogroup-horizontal", className), style, ...props }, children));
}
const RadioGroupItem = Radio;
Object.assign(__ds_scope, { Radio, RadioGroup, RadioGroupItem });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Radio.jsx", error: String((e && e.message) || e) }); }

// components/forms/SegmentedControl.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-seg{display:grid;grid-auto-flow:column;grid-auto-columns:minmax(0,1fr);gap:6px;font-family:var(--font-sans)}
.cr-seg-fit{display:inline-flex;flex-wrap:wrap;gap:6px;font-family:var(--font-sans)}
.cr-seg-item{display:inline-flex;align-items:center;justify-content:center;gap:6px;min-width:0;box-sizing:border-box;height:var(--control-md);padding:0 12px;border:1px solid var(--input);border-radius:var(--radius-full);background:var(--card);color:var(--muted-foreground);font-family:inherit;font-size:var(--text-sm);font-weight:var(--weight-medium);white-space:nowrap;cursor:pointer;transition:background var(--duration-fast) var(--ease-out),color var(--duration-fast) var(--ease-out),border-color var(--duration-fast) var(--ease-out)}
.cr-seg-item>span{overflow:hidden;text-overflow:ellipsis}
.cr-seg-item:hover:not(:disabled):not([aria-checked=true]){background:var(--accent);color:var(--foreground)}
.cr-seg-item[aria-checked=true]{border-color:var(--primary);background:var(--selection);color:var(--primary)}
.cr-seg-item:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-seg-item:disabled{opacity:.5;cursor:not-allowed}
.cr-seg-sm .cr-seg-item{height:var(--control-sm);padding:0 10px;font-size:var(--text-xs)}
.cr-seg-xl .cr-seg-item{height:var(--m-control);padding:0 14px;font-size:var(--m-text-secondary)}`;
const NEXT = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };
function keyNav(e) {
  if (!(e.key in NEXT) && e.key !== "Home" && e.key !== "End") return;
  const list = [...e.currentTarget.querySelectorAll("[role=radio]:not(:disabled)")];
  const i = list.indexOf(document.activeElement);
  if (i < 0) return;
  e.preventDefault();
  const n = e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : (i + NEXT[e.key] + list.length) % list.length;
  list[n].focus();
  list[n].click();
}
function SegmentedControl({ options, value, defaultValue, onValueChange, size = "md", fit = false, disabled = false, label, className = "", style, ...props }) {
  ensureCss("cr-seg-css", css);
  const norm = (options || []).map((o) => typeof o === "string" ? { value: o, label: o } : o);
  const [current, setCurrent] = useControllable(value, defaultValue != null ? defaultValue : norm[0] && norm[0].value, onValueChange);
  const s = sizeOf(size);
  const enabled = norm.filter((o) => !o.disabled);
  const tabStop = enabled.some((o) => o.value === current) ? current : enabled[0] && enabled[0].value;
  return /* @__PURE__ */ React.createElement("div", { role: "radiogroup", "aria-label": label, "aria-disabled": disabled || void 0, "data-slot": "segmented-control", onKeyDown: keyNav, className: cx(fit ? "cr-seg-fit" : "cr-seg", s !== "md" && `cr-seg-${s}`, className), style, ...props }, norm.map((o) => {
    const on = o.value === current;
    return /* @__PURE__ */ React.createElement("button", { key: o.value, type: "button", role: "radio", "aria-checked": on, tabIndex: o.value === tabStop ? 0 : -1, disabled: disabled || o.disabled, className: "cr-seg-item", onClick: () => setCurrent(o.value) }, o.icon && /* @__PURE__ */ React.createElement(Icon, { name: o.icon, size: s === "sm" ? 12 : s === "xl" ? 16 : 14 }), /* @__PURE__ */ React.createElement("span", null, o.label));
  }));
}
Object.assign(__ds_scope, { SegmentedControl });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/SegmentedControl.jsx", error: String((e && e.message) || e) }); }

// components/forms/RadioCard.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-rcgroup{display:grid;grid-template-columns:repeat(var(--cr-rc-columns,1),minmax(0,1fr));gap:6px;font-family:var(--font-sans)}
.cr-rc{display:flex;align-items:center;gap:10px;width:100%;box-sizing:border-box;padding:10px 12px;border:1px solid var(--input);border-radius:var(--radius-lg);background:var(--card);color:var(--card-foreground);font-family:inherit;text-align:left;cursor:pointer;transition:background var(--duration-fast) var(--ease-out),border-color var(--duration-fast) var(--ease-out),box-shadow var(--duration-fast) var(--ease-out)}
.cr-rc:hover:not(:disabled):not([aria-checked=true]){background:var(--accent)}
.cr-rc[aria-checked=true]{border-color:var(--primary);background:var(--selection);box-shadow:inset 0 0 0 1px var(--primary)}
.cr-rc:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-rc[aria-checked=true]:focus-visible{box-shadow:inset 0 0 0 1px var(--primary),0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-rc:disabled{opacity:.5;cursor:not-allowed}
.cr-rc-icon{display:flex;flex:none;color:var(--muted-foreground)}
.cr-rc[aria-checked=true] .cr-rc-icon{color:var(--primary)}
.cr-rc-text{display:flex;flex:1;flex-direction:column;gap:1px;min-width:0}
.cr-rc-title{font-size:var(--text-sm);font-weight:var(--weight-semibold);line-height:var(--leading-tight)}
.cr-rc-desc{font-size:var(--text-xs);color:var(--muted-foreground);line-height:var(--leading-normal)}
.cr-rc-meta{flex:none;font-family:var(--font-mono);font-size:var(--text-sm);font-variant-numeric:tabular-nums}
.cr-rcgroup-xl .cr-rc{min-height:var(--m-list-row);padding:12px 14px;gap:12px}
.cr-rcgroup-xl .cr-rc-title{font-size:var(--m-text-secondary)}
.cr-rcgroup-xl .cr-rc-desc{font-size:var(--m-text-footnote)}
.cr-rcgroup-xl .cr-rc-meta{font-size:var(--m-text-secondary)}`;
const GroupCtx = React.createContext(null);
const NEXT = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 };
function keyNav(e) {
  if (!(e.key in NEXT) && e.key !== "Home" && e.key !== "End") return;
  const list = [...e.currentTarget.querySelectorAll("[role=radio]:not(:disabled)")];
  const i = list.indexOf(document.activeElement);
  if (i < 0) return;
  e.preventDefault();
  const n = e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : (i + NEXT[e.key] + list.length) % list.length;
  list[n].focus();
  list[n].click();
}
function RadioCardGroup({ value, defaultValue, onValueChange, size = "md", columns = 1, disabled = false, label, children, className = "", style, ...props }) {
  ensureCss("cr-rc-css", css);
  const [current, setCurrent] = useControllable(value, defaultValue, onValueChange);
  const values = React.Children.toArray(children).filter((c) => React.isValidElement(c) && !c.props.disabled).map((c) => c.props.value);
  const tabStop = values.includes(current) ? current : values[0];
  return /* @__PURE__ */ React.createElement(GroupCtx.Provider, { value: { current, setCurrent, disabled, tabStop } }, /* @__PURE__ */ React.createElement("div", { role: "radiogroup", "aria-label": label, "data-slot": "radio-card-group", onKeyDown: keyNav, className: cx("cr-rcgroup", sizeOf(size) === "xl" && "cr-rcgroup-xl", className), style: { "--cr-rc-columns": columns, ...style }, ...props }, children));
}
function RadioCard({ value, icon, title, description, meta, disabled = false, children, className = "", ...props }) {
  ensureCss("cr-rc-css", css);
  const g = React.useContext(GroupCtx);
  const on = !!g && g.current === value;
  return /* @__PURE__ */ React.createElement("button", { type: "button", role: "radio", "aria-checked": on, tabIndex: g && g.tabStop === value ? 0 : -1, disabled: disabled || g && g.disabled, "data-slot": "radio-card", className: cx("cr-rc", className), onClick: () => g && g.setCurrent(value), ...props }, icon && /* @__PURE__ */ React.createElement("span", { className: "cr-rc-icon" }, /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 16 })), /* @__PURE__ */ React.createElement("span", { className: "cr-rc-text" }, /* @__PURE__ */ React.createElement("span", { className: "cr-rc-title" }, title), description && /* @__PURE__ */ React.createElement("span", { className: "cr-rc-desc" }, description), children), meta != null && /* @__PURE__ */ React.createElement("span", { className: "cr-rc-meta" }, meta));
}
Object.assign(__ds_scope, { RadioCardGroup, RadioCard });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/RadioCard.jsx", error: String((e && e.message) || e) }); }

// components/forms/Select.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-selectwrap{position:relative;display:inline-flex;align-items:center;width:100%}
.cr-select{appearance:none;width:100%;height:var(--control-md);padding:0 28px 0 10px;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground);background:var(--card);border:1px solid var(--input);border-radius:var(--radius-md);cursor:pointer;transition:border-color var(--duration-fast) var(--ease-out)}
.cr-select-sm{height:var(--control-sm);font-size:var(--text-xs)}
.cr-select-lg{height:var(--control-lg)}
.cr-select-xl{height:var(--m-control,44px);font-size:var(--m-text-body,15px);border-radius:var(--radius-lg)}
.cr-select:focus{outline:none;border-color:var(--ring);box-shadow:0 0 0 3px var(--selection)}
.cr-select:disabled{opacity:.5;cursor:not-allowed}
.cr-select[aria-invalid=true]{border-color:var(--destructive)}
.cr-selectchev{position:absolute;right:8px;color:var(--muted-foreground);pointer-events:none;display:flex}
.cr-selectfield{display:flex;flex-direction:column;gap:6px;font-family:var(--font-sans)}
.cr-selectfield>label{font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--foreground)}`;
const Select = React.forwardRef(function Select2({ label, options = [], value, defaultValue, onValueChange, onChange, disabled = false, placeholder, size = "md", id, children, style, className = "", ...props }, ref) {
  ensureCss("cr-select-css", css);
  const first = options.length ? typeof options[0] === "string" ? options[0] : options[0].value : "";
  const [v, setV] = useControllable(value, defaultValue != null ? defaultValue : placeholder ? "" : first, onValueChange);
  const auto = React.useId ? React.useId() : void 0;
  const fid = id || (label ? auto : void 0);
  const s = sizeOf(size);
  const sel = /* @__PURE__ */ React.createElement("span", { className: "cr-selectwrap", style }, /* @__PURE__ */ React.createElement("select", { ref, "data-slot": "select", id: fid, className: cx("cr-select", s !== "md" && "cr-select-" + s, className), value: v, onChange: (e) => {
    setV(e.target.value);
    onChange && onChange(e);
  }, disabled, ...props }, placeholder && /* @__PURE__ */ React.createElement("option", { value: "", disabled: true }, placeholder), options.map((o) => typeof o === "string" ? /* @__PURE__ */ React.createElement("option", { key: o, value: o }, o) : /* @__PURE__ */ React.createElement("option", { key: o.value, value: o.value, disabled: o.disabled }, o.label)), children), /* @__PURE__ */ React.createElement("span", { className: "cr-selectchev" }, /* @__PURE__ */ React.createElement(Icon, { name: "chevron-down", size: 14 })));
  if (!label) return sel;
  return /* @__PURE__ */ React.createElement("div", { className: "cr-selectfield" }, /* @__PURE__ */ React.createElement("label", { htmlFor: fid }, label), sel);
});
Object.assign(__ds_scope, { Select });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Select.jsx", error: String((e && e.message) || e) }); }

// components/forms/Switch.jsx
try { (() => {
const { ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-switch-label{position:relative;display:inline-flex;align-items:center;gap:8px;cursor:pointer;font-family:var(--font-sans);font-size:var(--text-sm);color:var(--foreground);user-select:none}
.cr-switch-label input{position:absolute;opacity:0;width:0;height:0}
.cr-switch{width:28px;height:16px;border-radius:var(--radius-full);background:var(--input);position:relative;transition:background var(--duration-base) var(--ease-out);flex:none}
.cr-switch::after{content:'';position:absolute;top:2px;left:2px;width:12px;height:12px;border-radius:var(--radius-full);background:#fff;box-shadow:var(--shadow-sm);transition:transform var(--duration-base) var(--ease-out)}
.cr-switch-label input:checked+.cr-switch{background:var(--primary)}
.cr-switch-label input:checked+.cr-switch::after{transform:translateX(12px)}
.cr-switch-label input:focus-visible+.cr-switch{box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-switch-disabled{opacity:.5;cursor:not-allowed}
/* xl (mobile) overrides stay last so they win over the base rules above */
.cr-switch-xl{min-height:var(--m-control,44px);gap:10px;font-size:var(--m-text-body,15px)}
.cr-switch-xl .cr-switch{width:40px;height:24px}
.cr-switch-xl .cr-switch::after{width:20px;height:20px}
.cr-switch-xl input:checked+.cr-switch::after{transform:translateX(16px)}`;
const Switch = React.forwardRef(function Switch2({ label, checked, defaultChecked = false, onCheckedChange, onChange, disabled = false, size = "md", style, className = "", ...props }, ref) {
  ensureCss("cr-switch-css", css);
  const [on, setOn] = useControllable(checked, defaultChecked, onCheckedChange);
  return /* @__PURE__ */ React.createElement("label", { "data-slot": "switch", className: cx("cr-switch-label", sizeOf(size) === "xl" && "cr-switch-xl", disabled && "cr-switch-disabled", className), style }, /* @__PURE__ */ React.createElement("input", { ref, type: "checkbox", role: "switch", "aria-checked": !!on, checked: !!on, onChange: (e) => {
    setOn(e.target.checked);
    onChange && onChange(e);
  }, disabled, ...props }), /* @__PURE__ */ React.createElement("span", { className: "cr-switch", "data-state": on ? "checked" : "unchecked" }), label && /* @__PURE__ */ React.createElement("span", null, label));
});
Object.assign(__ds_scope, { Switch });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/forms/Switch.jsx", error: String((e && e.message) || e) }); }

// components/navigation/Tabs.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf, useControllable } = __ds_scope;
const css = `.cr-tabsroot{display:flex;flex-direction:column;gap:12px;font-family:var(--font-sans)}
.cr-tabs{display:flex;gap:2px;font-family:var(--font-sans)}
.cr-tabs-underline{border-bottom:1px solid var(--border);gap:0}
.cr-tab{display:inline-flex;align-items:center;justify-content:center;gap:6px;min-width:0;border:none;background:none;cursor:pointer;font-family:inherit;font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--muted-foreground);white-space:nowrap;transition:color var(--duration-fast) var(--ease-out),background var(--duration-fast) var(--ease-out)}
.cr-tab:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}
.cr-tab:disabled{opacity:.5;cursor:not-allowed}
.cr-tabs-underline .cr-tab{padding:8px 12px;border-bottom:2px solid transparent;margin-bottom:-1px}
.cr-tabs-underline .cr-tab:hover{color:var(--foreground)}
.cr-tabs-underline .cr-tab-active{color:var(--foreground);border-bottom-color:var(--primary)}
.cr-tabs-pill{background:var(--muted);padding:2px;border-radius:var(--radius-md);display:inline-flex}
.cr-tabs-pill .cr-tab{padding:4px 10px;border-radius:var(--radius-sm)}
.cr-tabs-pill .cr-tab:hover{color:var(--foreground)}
.cr-tabs-pill .cr-tab-active{background:var(--tab-active,var(--card));color:var(--foreground);box-shadow:var(--shadow-sm)}
.cr-tabs-full{display:flex;width:100%}
.cr-tabs-full .cr-tab{flex:1 1 0}
.cr-tabs-xl .cr-tab{font-size:var(--m-text-secondary)}
.cr-tabs-underline.cr-tabs-xl .cr-tab{min-height:var(--m-control);padding:0 14px}
.cr-tabs-pill.cr-tabs-xl .cr-tab{min-height:calc(var(--m-control) - 4px);padding:0 12px}
.cr-tabcount{font-size:var(--text-xs);color:var(--muted-foreground)}`;
const Ctx = React.createContext(null);
function keyNav(e) {
  const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
  if (!keys.includes(e.key)) return;
  const list = [...e.currentTarget.querySelectorAll("[role=tab]:not(:disabled)")];
  const i = list.indexOf(document.activeElement);
  if (i < 0) return;
  e.preventDefault();
  const n = e.key === "Home" ? 0 : e.key === "End" ? list.length - 1 : (i + (e.key === "ArrowRight" ? 1 : -1) + list.length) % list.length;
  list[n].focus();
  list[n].click();
}
function listClass(variant, size, fullWidth, className) {
  return cx("cr-tabs", `cr-tabs-${variant}`, sizeOf(size) === "xl" && "cr-tabs-xl", fullWidth && "cr-tabs-full", className);
}
function Tabs({ items, value, defaultValue, onValueChange, onChange, variant = "underline", size = "md", fullWidth = false, children, style, className = "", ...props }) {
  ensureCss("cr-tabs-css", css);
  const norm = (items || []).map((it) => typeof it === "string" ? { id: it, label: it } : it);
  const handler = React.useCallback((v) => {
    onValueChange && onValueChange(v);
    onChange && onChange(v);
  }, [onValueChange, onChange]);
  const [active, setActive] = useControllable(value, defaultValue != null ? defaultValue : norm[0] && norm[0].id, handler);
  const iconSize = sizeOf(size) === "xl" ? 16 : 14;
  if (items) {
    return /* @__PURE__ */ React.createElement("div", { role: "tablist", "data-slot": "tabs-list", onKeyDown: keyNav, className: listClass(variant, size, fullWidth, className), style, ...props }, norm.map((o) => /* @__PURE__ */ React.createElement("button", { key: o.id, type: "button", role: "tab", tabIndex: active === o.id ? 0 : -1, "aria-selected": active === o.id, disabled: o.disabled, className: cx("cr-tab", active === o.id && "cr-tab-active"), onClick: () => setActive(o.id) }, o.icon && /* @__PURE__ */ React.createElement(Icon, { name: o.icon, size: iconSize }), o.label, o.count != null && /* @__PURE__ */ React.createElement("span", { className: "cr-tabcount" }, o.count))));
  }
  return /* @__PURE__ */ React.createElement(Ctx.Provider, { value: { active, setActive, variant, size, fullWidth } }, /* @__PURE__ */ React.createElement("div", { "data-slot": "tabs", className: cx("cr-tabsroot", className), style, ...props }, children));
}
function TabsList({ variant, size, fullWidth, className = "", children, ...props }) {
  ensureCss("cr-tabs-css", css);
  const c = React.useContext(Ctx);
  return /* @__PURE__ */ React.createElement("div", { role: "tablist", "data-slot": "tabs-list", onKeyDown: keyNav, className: listClass(variant || c && c.variant || "underline", size || c && c.size, fullWidth != null ? fullWidth : c && c.fullWidth, className), ...props }, children);
}
function TabsTrigger({ value, icon, count, disabled, className = "", children, ...props }) {
  const c = React.useContext(Ctx);
  const on = c && c.active === value;
  return /* @__PURE__ */ React.createElement("button", { type: "button", role: "tab", "data-slot": "tabs-trigger", "data-state": on ? "active" : "inactive", tabIndex: on ? 0 : -1, "aria-selected": !!on, disabled, className: cx("cr-tab", on && "cr-tab-active", className), onClick: () => c && c.setActive(value), ...props }, icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: c && sizeOf(c.size) === "xl" ? 16 : 14 }), children, count != null && /* @__PURE__ */ React.createElement("span", { className: "cr-tabcount" }, count));
}
function TabsContent({ value, className = "", children, ...props }) {
  const c = React.useContext(Ctx);
  if (!c || c.active !== value) return null;
  return /* @__PURE__ */ React.createElement("div", { role: "tabpanel", "data-slot": "tabs-content", className, ...props }, children);
}
Object.assign(__ds_scope, { Tabs, TabsList, TabsTrigger, TabsContent });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/Tabs.jsx", error: String((e && e.message) || e) }); }

// components/navigation/Pagination.jsx
try { (() => {
const { Icon, ensureCss, cx, sizeOf } = __ds_scope;
const css = `.cr-pagination{display:flex;align-items:center;gap:4px;font-family:var(--font-sans)}
.cr-page{display:inline-flex;align-items:center;justify-content:center;min-width:var(--control-md);height:var(--control-md);padding:0 8px;border:1px solid transparent;border-radius:var(--radius-md);background:none;font-family:inherit;font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--muted-foreground);cursor:pointer}
.cr-page:hover:not(:disabled){background:var(--accent);color:var(--foreground)}
.cr-page:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-page:disabled{opacity:.5;cursor:not-allowed}
.cr-page-active{background:var(--card);border-color:var(--input);color:var(--foreground);box-shadow:var(--shadow-sm)}
.cr-page-gap{min-width:20px;color:var(--muted-foreground);text-align:center;font-size:var(--text-sm)}
.cr-pagination-xl .cr-page{min-width:var(--m-control,44px);height:var(--m-control,44px);font-size:var(--m-text-body,15px)}`;
function pages(page, count, around) {
  const out = [1];
  const from = Math.max(2, page - around), to = Math.min(count - 1, page + around);
  if (from > 2) out.push("\u2026");
  for (let i = from; i <= to; i++) out.push(i);
  if (to < count - 1) out.push("\u2026");
  if (count > 1) out.push(count);
  return out;
}
function Pagination({ page = 1, pageCount = 1, onPageChange, siblings = 1, size = "md", labels = {}, className = "", style, ...props }) {
  ensureCss("cr-pagination-css", css);
  const L = { previous: "Previous page", next: "Next page", page: "Page", ...labels };
  const go = (p) => {
    if (p !== page && p >= 1 && p <= pageCount && onPageChange) onPageChange(p);
  };
  return /* @__PURE__ */ React.createElement("nav", { role: "navigation", "aria-label": "Pagination", "data-slot": "pagination", className: cx("cr-pagination", sizeOf(size) === "xl" && "cr-pagination-xl", className), style, ...props }, /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-page", "aria-label": L.previous, disabled: page <= 1, onClick: () => go(page - 1) }, /* @__PURE__ */ React.createElement(Icon, { name: "chevron-left", size: 16 })), pages(page, pageCount, siblings).map((p, i) => p === "\u2026" ? /* @__PURE__ */ React.createElement("span", { key: "g" + i, className: "cr-page-gap", "aria-hidden": "true" }, "\u2026") : /* @__PURE__ */ React.createElement("button", { key: p, type: "button", className: cx("cr-page", p === page && "cr-page-active"), "aria-label": `${L.page} ${p}`, "aria-current": p === page ? "page" : void 0, onClick: () => go(p) }, p)), /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-page", "aria-label": L.next, disabled: page >= pageCount, onClick: () => go(page + 1) }, /* @__PURE__ */ React.createElement(Icon, { name: "chevron-right", size: 16 })));
}
Object.assign(__ds_scope, { Pagination });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/Pagination.jsx", error: String((e && e.message) || e) }); }

// components/navigation/Sidebar.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-sidebar{display:flex;flex-direction:column;gap:2px;width:var(--sidebar-width);flex:none;box-sizing:border-box;padding:10px;background:var(--sidebar,var(--background));color:var(--sidebar-foreground,var(--foreground));border-right:1px solid var(--sidebar-border,var(--border));font-family:var(--font-sans);overflow:hidden}
.cr-sidebar-collapsed{width:var(--sidebar-collapsed-width,56px);align-items:stretch}
.cr-sidebar-head{display:flex;align-items:center;gap:8px;padding:4px 6px 10px;min-height:32px}
.cr-sidebar-foot{margin-top:auto;padding-top:6px}
.cr-sidebar-section{padding:12px 6px 4px;font-size:var(--text-xs);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted-foreground)}
.cr-sidebar-collapsed .cr-sidebar-section{height:1px;padding:8px 0 0;overflow:hidden;color:transparent}
.cr-navitem{display:flex;align-items:center;gap:8px;width:100%;box-sizing:border-box;height:30px;padding:0 8px;border:none;background:none;border-radius:var(--radius-md);font-family:inherit;font-size:var(--text-sm);font-weight:var(--weight-medium);color:var(--muted-foreground);text-align:left;text-decoration:none;cursor:pointer}
.cr-navitem:hover{background:var(--sidebar-accent,var(--accent));color:var(--foreground)}
.cr-navitem:focus-visible{outline:none;box-shadow:0 0 0 2px var(--background),0 0 0 4px var(--ring)}
.cr-navitem-active{background:var(--sidebar-accent,var(--accent));color:var(--sidebar-accent-foreground,var(--foreground))}
.cr-navitem-label{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cr-navitem-count{font-size:var(--text-xs);color:var(--muted-foreground)}
.cr-sidebar-collapsed .cr-navitem{justify-content:center;padding:0}
.cr-sidebar-collapsed .cr-navitem-label,.cr-sidebar-collapsed .cr-navitem-count{display:none}
@media (max-width:720px){.cr-sidebar{display:none}}`;
function Sidebar({ collapsed = false, header, footer, label = "Main", children, className = "", style, ...props }) {
  ensureCss("cr-sidebar-css", css);
  return /* @__PURE__ */ React.createElement("nav", { "aria-label": label, "data-slot": "sidebar", "data-state": collapsed ? "collapsed" : "expanded", className: cx("cr-sidebar", collapsed && "cr-sidebar-collapsed", className), style, ...props }, header && /* @__PURE__ */ React.createElement("div", { className: "cr-sidebar-head" }, header), children, footer && /* @__PURE__ */ React.createElement("div", { className: "cr-sidebar-foot" }, footer));
}
function SidebarSection({ label, className = "", children, ...props }) {
  return /* @__PURE__ */ React.createElement(React.Fragment, null, label && /* @__PURE__ */ React.createElement("div", { "data-slot": "sidebar-section", className: cx("cr-sidebar-section", className), ...props }, label), children);
}
function SidebarItem({ icon, count, active = false, href, children, className = "", ...props }) {
  const Tag = href ? "a" : "button";
  return /* @__PURE__ */ React.createElement(
    Tag,
    {
      "data-slot": "sidebar-item",
      href,
      type: href ? void 0 : "button",
      "aria-current": active ? "page" : void 0,
      title: typeof children === "string" ? children : void 0,
      className: cx("cr-navitem", active && "cr-navitem-active", className),
      ...props
    },
    icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 16 }),
    /* @__PURE__ */ React.createElement("span", { className: "cr-navitem-label" }, children),
    count != null && /* @__PURE__ */ React.createElement("span", { className: "cr-navitem-count" }, count)
  );
}
Object.assign(__ds_scope, { Sidebar, SidebarSection, SidebarItem });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/Sidebar.jsx", error: String((e && e.message) || e) }); }

// components/navigation/List.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-list{display:flex;flex-direction:column;font-family:var(--font-sans);border-radius:var(--radius-md);overflow:hidden}
.cr-listrow{display:flex;align-items:center;gap:10px;width:100%;box-sizing:border-box;min-height:38px;padding:0 12px;border:none;border-bottom:1px solid var(--border);background:none;font-family:inherit;font-size:var(--text-sm);color:var(--foreground);text-align:left;cursor:pointer}
.cr-listrow:last-child{border-bottom:none}
.cr-listrow:hover{background:var(--accent)}
.cr-listrow:focus-visible{outline:none;box-shadow:inset 0 0 0 2px var(--ring)}
.cr-listrow-selected{background:var(--accent)}
.cr-listrow-title{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:var(--weight-medium)}
.cr-listrow-meta{display:inline-flex;align-items:center;gap:10px;color:var(--muted-foreground);font-size:var(--text-xs);white-space:nowrap}
.cr-listrow-actions{display:inline-flex;align-items:center;gap:2px;opacity:0}
.cr-listrow:hover .cr-listrow-actions,.cr-listrow:focus-within .cr-listrow-actions{opacity:1}
@media (hover:none){.cr-listrow-actions{opacity:1}.cr-listrow{min-height:var(--m-list-row,44px)}}`;
const KEYS = { ArrowDown: 1, ArrowUp: -1, j: 1, k: -1 };
function List({ selectable = false, vimKeys = false, label, children, className = "", style, ...props }) {
  ensureCss("cr-list-css", css);
  const ref = React.useRef(null);
  const onKeyDown = (e) => {
    const dir = KEYS[e.key];
    const vim = e.key === "j" || e.key === "k";
    if (vim && !vimKeys || !dir && e.key !== "Home" && e.key !== "End") return;
    const rows = [...ref.current.querySelectorAll("[data-slot=list-item]:not(:disabled)")];
    if (!rows.length) return;
    e.preventDefault();
    const i = rows.indexOf(document.activeElement);
    const n = e.key === "Home" ? 0 : e.key === "End" ? rows.length - 1 : i < 0 ? 0 : Math.min(rows.length - 1, Math.max(0, i + dir));
    rows[n].focus();
  };
  return /* @__PURE__ */ React.createElement("div", { ref, role: selectable ? "listbox" : "list", "aria-label": label, "data-slot": "list", onKeyDown, className: cx("cr-list", className), style, ...props }, children);
}
function ListItem({ icon, iconColor, title, meta, actions, leading, selected = false, disabled = false, onActivate, onClick, children, className = "", ...props }) {
  const activate = (e) => {
    onClick && onClick(e);
    onActivate && onActivate(e);
  };
  return /* @__PURE__ */ React.createElement(
    "div",
    {
      role: props.role || "listitem",
      tabIndex: disabled ? -1 : 0,
      "data-slot": "list-item",
      "aria-selected": selected || void 0,
      "aria-disabled": disabled || void 0,
      className: cx("cr-listrow", selected && "cr-listrow-selected", className),
      onClick: disabled ? void 0 : activate,
      onKeyDown: (e) => {
        if (!disabled && (e.key === "Enter" || e.key === " ")) {
          e.preventDefault();
          activate(e);
        }
      },
      ...props
    },
    leading,
    icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 16, color: iconColor }),
    /* @__PURE__ */ React.createElement("span", { className: "cr-listrow-title" }, title || children),
    meta && /* @__PURE__ */ React.createElement("span", { className: "cr-listrow-meta" }, meta),
    actions && /* @__PURE__ */ React.createElement("span", { className: "cr-listrow-actions", onClick: (e) => e.stopPropagation() }, actions)
  );
}
Object.assign(__ds_scope, { List, ListItem });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/navigation/List.jsx", error: String((e && e.message) || e) }); }

// components/overlay/Popover.jsx
try { (() => {
const { ensureCss, cx, useControllable } = __ds_scope;
const css = `.cr-pop-anchor{position:relative;display:inline-flex}
.cr-pop{position:absolute;z-index:115;min-width:180px;padding:8px;background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-radius:var(--radius-lg);box-shadow:var(--shadow-popover);font-family:var(--font-sans);font-size:var(--text-sm);animation:cr-pop-in var(--duration-fast) var(--ease-out)}
.cr-pop-bottom{top:calc(100% + 6px)}
.cr-pop-top{bottom:calc(100% + 6px)}
.cr-pop-start{left:0}
.cr-pop-end{right:0}
.cr-pop-center{left:50%;transform:translateX(-50%)}
@keyframes cr-pop-in{from{opacity:0;transform:translateY(-4px)}}`;
const Ctx = React.createContext(null);
function Popover({ open, defaultOpen = false, onOpenChange, side = "bottom", align = "start", children, className = "", style, ...props }) {
  ensureCss("cr-pop-css", css);
  const [on, setOn] = useControllable(open, defaultOpen, onOpenChange);
  const ref = React.useRef(null);
  React.useEffect(() => {
    if (!on) return;
    const away = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOn(false);
    };
    const key = (e) => {
      if (e.key === "Escape") setOn(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", key);
    };
  }, [on, setOn]);
  return /* @__PURE__ */ React.createElement(Ctx.Provider, { value: { on, setOn, side, align } }, /* @__PURE__ */ React.createElement("span", { ref, "data-slot": "popover", className: cx("cr-pop-anchor", className), style, ...props }, children));
}
function PopoverTrigger({ children, asChild = true }) {
  const c = React.useContext(Ctx);
  const toggle = (e) => {
    e.preventDefault();
    c && c.setOn(!c.on);
  };
  if (asChild && React.isValidElement(children)) return React.cloneElement(children, { "data-slot": "popover-trigger", "aria-expanded": !!(c && c.on), "aria-haspopup": "dialog", onClick: toggle });
  return /* @__PURE__ */ React.createElement("button", { type: "button", "data-slot": "popover-trigger", "aria-expanded": !!(c && c.on), onClick: toggle }, children);
}
function PopoverContent({ children, className = "", style, ...props }) {
  const c = React.useContext(Ctx);
  if (!c || !c.on) return null;
  return /* @__PURE__ */ React.createElement("div", { role: "dialog", "data-slot": "popover-content", className: cx("cr-pop", `cr-pop-${c.side}`, `cr-pop-${c.align}`, className), style, ...props }, children);
}
Object.assign(__ds_scope, { Popover, PopoverTrigger, PopoverContent });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/Popover.jsx", error: String((e && e.message) || e) }); }

// components/overlay/DropdownMenu.jsx
try { (() => {
const { Icon, ensureCss, cx, useControllable } = __ds_scope;
const css = `.cr-menu-anchor{position:relative;display:inline-flex}
.cr-menu{position:absolute;z-index:115;min-width:190px;padding:4px;background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-radius:var(--radius-lg);box-shadow:var(--shadow-popover);font-family:var(--font-sans);animation:cr-menu-in var(--duration-fast) var(--ease-out)}
.cr-menu-bottom{top:calc(100% + 6px)}
.cr-menu-top{bottom:calc(100% + 6px)}
.cr-menu-start{left:0}
.cr-menu-end{right:0}
.cr-menu-item{display:flex;align-items:center;gap:8px;width:100%;box-sizing:border-box;height:30px;padding:0 8px;border:none;background:none;border-radius:var(--radius-sm);font-family:inherit;font-size:var(--text-sm);color:var(--foreground);text-align:left;cursor:pointer}
.cr-menu-item:hover:not(:disabled),.cr-menu-item:focus-visible{background:var(--accent);outline:none}
.cr-menu-item:disabled{opacity:.5;cursor:not-allowed}
.cr-menu-item-destructive{color:var(--destructive)}
.cr-menu-shortcut{margin-left:auto;font-family:var(--font-mono);font-size:var(--text-xs);color:var(--muted-foreground)}
.cr-menu-label{padding:6px 8px 4px;font-size:var(--text-xs);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted-foreground)}
.cr-menu-sep{height:1px;margin:4px -4px;background:var(--border)}
@keyframes cr-menu-in{from{opacity:0;transform:translateY(-4px)}}`;
const Ctx = React.createContext(null);
function DropdownMenu({ open, defaultOpen = false, onOpenChange, side = "bottom", align = "start", children, className = "", style, ...props }) {
  ensureCss("cr-menu-css", css);
  const [on, setOn] = useControllable(open, defaultOpen, onOpenChange);
  const ref = React.useRef(null);
  React.useEffect(() => {
    if (!on) return;
    const away = (e) => {
      if (ref.current && !ref.current.contains(e.target)) setOn(false);
    };
    const key = (e) => {
      if (e.key === "Escape") setOn(false);
    };
    document.addEventListener("mousedown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", away);
      document.removeEventListener("keydown", key);
    };
  }, [on, setOn]);
  return /* @__PURE__ */ React.createElement(Ctx.Provider, { value: { on, setOn, side, align } }, /* @__PURE__ */ React.createElement("span", { ref, "data-slot": "dropdown-menu", className: cx("cr-menu-anchor", className), style, ...props }, children));
}
function DropdownMenuTrigger({ children, asChild = true }) {
  const c = React.useContext(Ctx);
  const toggle = (e) => {
    e.preventDefault();
    c && c.setOn(!c.on);
  };
  if (asChild && React.isValidElement(children)) return React.cloneElement(children, { "data-slot": "dropdown-menu-trigger", "aria-haspopup": "menu", "aria-expanded": !!(c && c.on), onClick: toggle });
  return /* @__PURE__ */ React.createElement("button", { type: "button", "data-slot": "dropdown-menu-trigger", "aria-haspopup": "menu", "aria-expanded": !!(c && c.on), onClick: toggle }, children);
}
function move(e) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  const list = [...e.currentTarget.querySelectorAll("[role=menuitem]:not(:disabled)")];
  if (!list.length) return;
  e.preventDefault();
  const i = list.indexOf(document.activeElement);
  list[(i + (e.key === "ArrowDown" ? 1 : -1) + list.length + (i < 0 ? 1 : 0)) % list.length].focus();
}
function DropdownMenuContent({ items, children, className = "", style, ...props }) {
  const c = React.useContext(Ctx);
  const ref = React.useRef(null);
  React.useEffect(() => {
    if (c && c.on && ref.current) {
      const f = ref.current.querySelector("[role=menuitem]:not(:disabled)");
      f && f.focus();
    }
  }, [c && c.on]);
  if (!c || !c.on) return null;
  return /* @__PURE__ */ React.createElement("div", { ref, role: "menu", "data-slot": "dropdown-menu-content", onKeyDown: move, className: cx("cr-menu", `cr-menu-${c.side}`, `cr-menu-${c.align}`, className), style, ...props }, items ? items.map((it, i) => it === "separator" || it.separator ? /* @__PURE__ */ React.createElement(DropdownMenuSeparator, { key: i }) : it.label && it.items === void 0 && it.header ? /* @__PURE__ */ React.createElement(DropdownMenuLabel, { key: i }, it.label) : /* @__PURE__ */ React.createElement(DropdownMenuItem, { key: i, icon: it.icon, shortcut: it.shortcut, disabled: it.disabled, variant: it.variant, onSelect: it.onSelect }, it.label)) : children);
}
function DropdownMenuItem({ icon, shortcut, variant = "default", onSelect, onClick, children, className = "", ...props }) {
  const c = React.useContext(Ctx);
  return /* @__PURE__ */ React.createElement(
    "button",
    {
      type: "button",
      role: "menuitem",
      "data-slot": "dropdown-menu-item",
      className: cx("cr-menu-item", variant === "destructive" && "cr-menu-item-destructive", className),
      onClick: (e) => {
        onClick && onClick(e);
        onSelect && onSelect(e);
        c && c.setOn(false);
      },
      ...props
    },
    icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 15 }),
    /* @__PURE__ */ React.createElement("span", null, children),
    shortcut && /* @__PURE__ */ React.createElement("span", { className: "cr-menu-shortcut" }, shortcut)
  );
}
function DropdownMenuCheckboxItem({ checked = false, onCheckedChange, icon, children, className = "", ...props }) {
  return /* @__PURE__ */ React.createElement(
    "button",
    {
      type: "button",
      role: "menuitemcheckbox",
      "aria-checked": !!checked,
      "data-slot": "dropdown-menu-checkbox-item",
      className: cx("cr-menu-item", className),
      onClick: () => onCheckedChange && onCheckedChange(!checked),
      ...props
    },
    /* @__PURE__ */ React.createElement(Icon, { name: checked ? "check" : "point", size: 15, style: { opacity: checked ? 1 : 0 } }),
    /* @__PURE__ */ React.createElement("span", null, children)
  );
}
function DropdownMenuLabel({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("div", { "data-slot": "dropdown-menu-label", className: cx("cr-menu-label", className), ...p });
}
function DropdownMenuSeparator({ className = "", ...p }) {
  return /* @__PURE__ */ React.createElement("div", { role: "separator", "data-slot": "dropdown-menu-separator", className: cx("cr-menu-sep", className), ...p });
}
Object.assign(__ds_scope, { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem, DropdownMenuCheckboxItem, DropdownMenuLabel, DropdownMenuSeparator });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/DropdownMenu.jsx", error: String((e && e.message) || e) }); }

// components/overlay/CommandPalette.jsx
try { (() => {
const { Icon, Kbd, ensureCss, cx } = __ds_scope;
const css = `.cr-cmd-overlay{position:fixed;inset:0;background:rgba(0,0,0,.5);backdrop-filter:blur(4px);display:flex;align-items:flex-start;justify-content:center;padding-top:12vh;z-index:130;animation:cr-fade var(--duration-base) var(--ease-out)}
.cr-cmd{width:min(560px,calc(100vw - 32px));max-height:min(60vh,440px);display:flex;flex-direction:column;background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-radius:var(--radius-xl);box-shadow:var(--shadow-lg);font-family:var(--font-sans);overflow:hidden;animation:cr-pop var(--duration-base) var(--ease-out)}
.cr-cmd-search{display:flex;align-items:center;gap:8px;padding:0 12px;height:44px;border-bottom:1px solid var(--border);flex:none}
.cr-cmd-input{flex:1;border:none;background:none;outline:none;font-family:inherit;font-size:var(--text-base);color:var(--foreground)}
.cr-cmd-input::placeholder{color:var(--muted-foreground)}
.cr-cmd-list{overflow:auto;padding:6px;flex:1}
.cr-cmd-group{padding:8px 8px 4px;font-size:var(--text-xs);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted-foreground)}
.cr-cmd-item{display:flex;align-items:center;gap:8px;width:100%;box-sizing:border-box;height:34px;padding:0 8px;border:none;background:none;border-radius:var(--radius-md);font-family:inherit;font-size:var(--text-sm);color:var(--foreground);text-align:left;cursor:pointer}
.cr-cmd-item[data-active=true]{background:var(--accent)}
.cr-cmd-item:disabled{opacity:.5;cursor:not-allowed}
.cr-cmd-item-label{flex:1;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cr-cmd-item-hint{color:var(--muted-foreground);font-size:var(--text-xs)}
.cr-cmd-empty{padding:28px 12px;text-align:center;font-size:var(--text-sm);color:var(--muted-foreground)}
.cr-cmd-foot{display:flex;align-items:center;gap:12px;padding:6px 12px;border-top:1px solid var(--border);font-size:var(--text-xs);color:var(--muted-foreground);flex:none}
@keyframes cr-fade{from{opacity:0}}
@keyframes cr-pop{from{opacity:0;transform:translateY(-8px)}}`;
const flat = (groups) => groups.flatMap((g) => (g.items || []).map((it) => ({ ...it, group: g.group })));
const match = (it, q) => {
  if (!q) return true;
  const hay = [it.label, it.group, it.hint, ...it.keywords || []].filter(Boolean).join(" ").toLowerCase();
  return q.toLowerCase().split(/\s+/).every((w) => hay.includes(w));
};
function CommandPalette({ open = false, onOpenChange, groups = [], placeholder = "Type a command or search\u2026", emptyLabel = "No matches", hotkey = "k", footer, className = "", style, ...props }) {
  ensureCss("cr-cmd-css", css);
  const [q, setQ] = React.useState("");
  const [active, setActive] = React.useState(0);
  const listRef = React.useRef(null);
  const items = React.useMemo(() => flat(groups).filter((it) => match(it, q)), [groups, q]);
  React.useEffect(() => {
    setActive(0);
  }, [q, open]);
  React.useEffect(() => {
    if (!open) setQ("");
  }, [open]);
  React.useEffect(() => {
    if (!hotkey || !onOpenChange) return;
    const onKey = (e) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === hotkey) {
        e.preventDefault();
        onOpenChange(true);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [hotkey, onOpenChange]);
  React.useEffect(() => {
    const el = listRef.current && listRef.current.querySelector("[data-active=true]");
    el && el.scrollIntoView({ block: "nearest" });
  }, [active, items.length]);
  if (!open) return null;
  const close = () => onOpenChange && onOpenChange(false);
  const run = (it) => {
    if (!it || it.disabled) return;
    close();
    it.onSelect && it.onSelect(it);
  };
  const onKeyDown = (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      setActive((a) => (a + (e.key === "ArrowDown" ? 1 : -1) + items.length) % Math.max(items.length, 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(items[active]);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };
  let seen = null;
  return /* @__PURE__ */ React.createElement("div", { className: "cr-cmd-overlay", "data-slot": "command-overlay", onClick: (e) => {
    if (e.target === e.currentTarget) close();
  } }, /* @__PURE__ */ React.createElement("div", { role: "dialog", "aria-modal": "true", "aria-label": "Command palette", "data-slot": "command", className: cx("cr-cmd", className), style, ...props }, /* @__PURE__ */ React.createElement("div", { className: "cr-cmd-search" }, /* @__PURE__ */ React.createElement(Icon, { name: "search", size: 16, color: "var(--muted-foreground)" }), /* @__PURE__ */ React.createElement(
    "input",
    {
      autoFocus: true,
      className: "cr-cmd-input",
      role: "combobox",
      "aria-expanded": "true",
      "aria-controls": "cr-cmd-list",
      "aria-autocomplete": "list",
      placeholder,
      value: q,
      onChange: (e) => setQ(e.target.value),
      onKeyDown
    }
  ), /* @__PURE__ */ React.createElement(Kbd, { keys: ["Esc"] })), /* @__PURE__ */ React.createElement("div", { id: "cr-cmd-list", ref: listRef, role: "listbox", className: "cr-cmd-list" }, items.length === 0 && /* @__PURE__ */ React.createElement("p", { className: "cr-cmd-empty" }, emptyLabel), items.map((it, i) => {
    var _a;
    const head = it.group && it.group !== seen ? seen = it.group : null;
    return /* @__PURE__ */ React.createElement(React.Fragment, { key: (_a = it.id) != null ? _a : it.label }, head && /* @__PURE__ */ React.createElement("div", { className: "cr-cmd-group" }, head), /* @__PURE__ */ React.createElement(
      "button",
      {
        type: "button",
        role: "option",
        "aria-selected": i === active,
        "data-active": i === active,
        disabled: it.disabled,
        className: "cr-cmd-item",
        onMouseMove: () => setActive(i),
        onClick: () => run(it)
      },
      it.icon && /* @__PURE__ */ React.createElement(Icon, { name: it.icon, size: 15, color: "var(--muted-foreground)" }),
      /* @__PURE__ */ React.createElement("span", { className: "cr-cmd-item-label" }, it.label),
      it.hint && /* @__PURE__ */ React.createElement("span", { className: "cr-cmd-item-hint" }, it.hint),
      it.shortcut && /* @__PURE__ */ React.createElement(Kbd, { keys: Array.isArray(it.shortcut) ? it.shortcut : [it.shortcut] })
    ));
  })), /* @__PURE__ */ React.createElement("div", { className: "cr-cmd-foot" }, footer || /* @__PURE__ */ React.createElement(React.Fragment, null, /* @__PURE__ */ React.createElement("span", null, "\u2191\u2193 to move"), /* @__PURE__ */ React.createElement("span", null, "\u21B5 to run"), /* @__PURE__ */ React.createElement("span", null, "esc to close")))));
}
Object.assign(__ds_scope, { CommandPalette });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/CommandPalette.jsx", error: String((e && e.message) || e) }); }

// components/overlay/BottomSheet.jsx
try { (() => {
const { Icon, ensureCss, cx } = __ds_scope;
const css = `.cr-sheet-overlay{position:fixed;inset:0;background:rgba(0,0,0,.5);backdrop-filter:blur(4px);display:flex;align-items:flex-end;justify-content:center;z-index:100;animation:cr-fade var(--duration-base) var(--ease-out)}
.cr-sheet{background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-bottom:none;border-radius:var(--m-sheet-radius) var(--m-sheet-radius) 0 0;box-shadow:var(--shadow-lg);width:100%;max-width:560px;max-height:85%;display:flex;flex-direction:column;font-family:var(--font-sans);animation:cr-sheet-up var(--duration-slow) var(--ease-out);transition:transform var(--duration-slow) var(--ease-out)}
.cr-sheet-dragging{transition:none}
.cr-sheet:focus{outline:none}
.cr-sheet-grab{display:flex;justify-content:center;padding:8px 0 6px;touch-action:none;cursor:grab}
.cr-sheet-grab span{width:36px;height:4px;border-radius:var(--radius-full);background:var(--input)}
.cr-sheet-head{display:grid;grid-template-columns:1fr minmax(0,auto) 1fr;align-items:center;min-height:var(--m-navbar-height);padding:2px 4px;border-bottom:1px solid var(--border)}
.cr-sheet-dismissible .cr-sheet-head{min-height:0;padding:0 var(--m-inset) 10px}
.cr-sheet-title{font-size:var(--m-text-title);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-heading);line-height:var(--leading-tight);margin:0;text-align:center;grid-column:2;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.cr-sheet-icon{display:inline-flex;align-items:center;justify-content:center;width:var(--m-control);height:var(--m-control);border:none;background:none;border-radius:var(--radius-md);color:var(--foreground);cursor:pointer;transition:background var(--duration-fast) var(--ease-out)}
.cr-sheet-icon:hover:not(:disabled){background:var(--accent)}
.cr-sheet-icon:focus-visible{outline:none;box-shadow:0 0 0 2px var(--ring)}
.cr-sheet-icon:disabled{opacity:.4;cursor:not-allowed}
.cr-sheet-cancel{justify-self:start;grid-column:1}
.cr-sheet-confirm{justify-self:end;grid-column:3;color:var(--primary)}
.cr-sheet-spin{animation:cr-sheet-spin 1s linear infinite}
.cr-sheet-body{padding:16px var(--m-inset) calc(16px + env(safe-area-inset-bottom));overflow:auto;font-size:var(--m-text-secondary)}
@keyframes cr-sheet-up{from{opacity:0;transform:translateY(24px)}}
@keyframes cr-sheet-spin{to{transform:rotate(360deg)}}
@keyframes cr-fade{from{opacity:0}}`;
const CLOSE_DISTANCE = 80;
const CLOSE_VELOCITY = 0.5;
function BottomSheet({ open, onOpenChange, title, dismissible, onCancel, onConfirm, cancelLabel = "Cancel", confirmLabel = "Save", confirmDisabled = false, confirmLoading = false, children, style, className = "", ...props }) {
  ensureCss("cr-sheet-css", css);
  const panel = React.useRef(null);
  const drag = React.useRef(null);
  const [offset, setOffset] = React.useState(0);
  const ids = React.useId ? React.useId() : "sheet";
  const free = dismissible != null ? dismissible : !onConfirm;
  const close = React.useCallback(() => {
    onOpenChange && onOpenChange(false);
  }, [onOpenChange]);
  const cancel = React.useCallback(() => {
    if (onCancel) onCancel();
    else close();
  }, [onCancel, close]);
  React.useEffect(() => {
    if (!open) return;
    setOffset(0);
    const prev = document.activeElement;
    const t = setTimeout(() => {
      const f = panel.current && panel.current.querySelector(".cr-sheet-body input,.cr-sheet-body select,.cr-sheet-body textarea");
      (f || panel.current) && (f || panel.current).focus();
    }, 0);
    const onKey = (e) => {
      if (e.key === "Escape" && free) close();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      clearTimeout(t);
      document.removeEventListener("keydown", onKey);
      prev && prev.focus && prev.focus();
    };
  }, [open, free, close]);
  if (!open) return null;
  const grabDown = (e) => {
    drag.current = { y: e.clientY, t: Date.now() };
    e.currentTarget.setPointerCapture && e.currentTarget.setPointerCapture(e.pointerId);
  };
  const grabMove = (e) => {
    if (drag.current) setOffset(Math.max(0, e.clientY - drag.current.y));
  };
  const grabUp = (e) => {
    if (!drag.current) return;
    const d = Math.max(0, e.clientY - drag.current.y);
    const v = d / Math.max(1, Date.now() - drag.current.t);
    drag.current = null;
    if (d > CLOSE_DISTANCE || v > CLOSE_VELOCITY) close();
    else setOffset(0);
  };
  return /* @__PURE__ */ React.createElement("div", { className: "cr-sheet-overlay", "data-slot": "drawer-overlay", onClick: (e) => {
    if (e.target === e.currentTarget && free) close();
  } }, /* @__PURE__ */ React.createElement("div", { ref: panel, tabIndex: -1, role: "dialog", "aria-modal": "true", "aria-labelledby": title ? ids + "-t" : void 0, "data-slot": "drawer-content", className: cx("cr-sheet", free && "cr-sheet-dismissible", drag.current && "cr-sheet-dragging", className), style: { ...style, ...offset ? { transform: `translateY(${offset}px)` } : null }, ...props }, free && /* @__PURE__ */ React.createElement("div", { className: "cr-sheet-grab", "aria-hidden": "true", onPointerDown: grabDown, onPointerMove: grabMove, onPointerUp: grabUp, onPointerCancel: grabUp }, /* @__PURE__ */ React.createElement("span", null)), /* @__PURE__ */ React.createElement("div", { className: "cr-sheet-head" }, !free && /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-sheet-icon cr-sheet-cancel", "aria-label": cancelLabel, onClick: cancel }, /* @__PURE__ */ React.createElement(Icon, { name: "x", size: 24 })), /* @__PURE__ */ React.createElement("h2", { id: ids + "-t", className: "cr-sheet-title" }, title), !free && onConfirm && /* @__PURE__ */ React.createElement("button", { type: "button", className: "cr-sheet-icon cr-sheet-confirm", "aria-label": confirmLabel, "aria-busy": confirmLoading || void 0, disabled: confirmDisabled || confirmLoading, onClick: onConfirm }, confirmLoading ? /* @__PURE__ */ React.createElement(Icon, { name: "loader-2", size: 24, className: "cr-sheet-spin" }) : /* @__PURE__ */ React.createElement(Icon, { name: "check", size: 24 }))), /* @__PURE__ */ React.createElement("div", { className: "cr-sheet-body" }, children)));
}
Object.assign(__ds_scope, { BottomSheet });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/BottomSheet.jsx", error: String((e && e.message) || e) }); }

// components/overlay/Dialog.jsx
try { (() => {
const { Icon, IconButton, ensureCss, cx } = __ds_scope;
const css = `.cr-dialog-overlay{position:fixed;inset:0;background:rgba(0,0,0,.5);backdrop-filter:blur(4px);display:flex;align-items:flex-start;justify-content:center;padding-top:14vh;z-index:100;animation:cr-fade var(--duration-base) var(--ease-out)}
.cr-dialog{background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-radius:var(--radius-xl);box-shadow:var(--shadow-lg);width:min(440px,calc(100vw - 32px));font-family:var(--font-sans);animation:cr-pop var(--duration-base) var(--ease-out)}
.cr-dialog:focus{outline:none}
.cr-dialog-head{display:flex;align-items:flex-start;gap:10px;padding:16px 16px 14px}
.cr-dialog-title{font-size:var(--text-base);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-heading);margin:0;flex:1}
.cr-dialog-desc{font-size:var(--text-sm);color:var(--muted-foreground);margin:4px 0 0}
.cr-dialog-body{padding:2px 16px 16px;font-size:var(--text-sm)}
.cr-dialog-foot{padding:12px 16px;border-top:1px solid var(--border);display:flex;justify-content:flex-end;gap:8px}
@keyframes cr-fade{from{opacity:0}}
@keyframes cr-pop{from{opacity:0;transform:translateY(-8px)}}`;
function Dialog({ open, onOpenChange, onClose, title, description, icon, footer, closeLabel = "Close", children, style, className = "", ...props }) {
  ensureCss("cr-dialog-css", css);
  const panel = React.useRef(null);
  const ids = React.useId ? React.useId() : "dlg";
  const close = React.useCallback(() => {
    onOpenChange && onOpenChange(false);
    onClose && onClose();
  }, [onOpenChange, onClose]);
  const canClose = !!(onOpenChange || onClose);
  React.useEffect(() => {
    if (!open) return;
    const prev = document.activeElement;
    const t = setTimeout(() => {
      const f = panel.current && panel.current.querySelector('input,select,textarea,button:not([data-close]),[tabindex]:not([tabindex="-1"])');
      (f || panel.current) && (f || panel.current).focus();
    }, 0);
    const onKey = (e) => {
      if (e.key === "Escape" && canClose) close();
    };
    document.addEventListener("keydown", onKey);
    return () => {
      clearTimeout(t);
      document.removeEventListener("keydown", onKey);
      prev && prev.focus && prev.focus();
    };
  }, [open, close, canClose]);
  if (!open) return null;
  return /* @__PURE__ */ React.createElement("div", { className: "cr-dialog-overlay", "data-slot": "dialog-overlay", onClick: (e) => {
    if (e.target === e.currentTarget && canClose) close();
  } }, /* @__PURE__ */ React.createElement("div", { ref: panel, tabIndex: -1, role: "dialog", "aria-modal": "true", "aria-labelledby": title ? ids + "-t" : void 0, "aria-describedby": description ? ids + "-d" : void 0, "data-slot": "dialog-content", className: cx("cr-dialog", className), style, ...props }, (title || description || canClose) && /* @__PURE__ */ React.createElement("div", { className: "cr-dialog-head" }, icon && /* @__PURE__ */ React.createElement(Icon, { name: icon, size: 18, style: { marginTop: 2, color: "var(--muted-foreground)" } }), /* @__PURE__ */ React.createElement("div", { style: { flex: 1 } }, title && /* @__PURE__ */ React.createElement("h2", { id: ids + "-t", className: "cr-dialog-title" }, title), description && /* @__PURE__ */ React.createElement("p", { id: ids + "-d", className: "cr-dialog-desc" }, description)), canClose && /* @__PURE__ */ React.createElement(IconButton, { "data-close": "", icon: "x", label: closeLabel, size: "sm", onClick: close })), children && /* @__PURE__ */ React.createElement("div", { className: "cr-dialog-body" }, children), footer && /* @__PURE__ */ React.createElement("div", { className: "cr-dialog-foot" }, footer)));
}
Object.assign(__ds_scope, { Dialog });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/Dialog.jsx", error: String((e && e.message) || e) }); }

// components/overlay/Toast.jsx
try { (() => {
const { Icon, IconButton, ensureCss, cx, status, isUrgent } = __ds_scope;
const css = `.cr-toast{display:flex;align-items:flex-start;gap:10px;width:min(360px,calc(100vw - 32px));box-sizing:border-box;padding:12px 12px 12px 14px;background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);border-radius:var(--radius-lg);box-shadow:var(--shadow-lg);font-family:var(--font-sans);animation:cr-toast-in var(--duration-slow) var(--ease-out)}
.cr-toast-title{font-size:var(--text-sm);font-weight:var(--weight-medium);margin:0}
.cr-toast-desc{font-size:var(--text-xs);color:var(--muted-foreground);margin:2px 0 0}
.cr-toaster{position:fixed;z-index:120;display:flex;flex-direction:column;gap:8px;pointer-events:none}
.cr-toaster>*{pointer-events:auto}
.cr-toaster-bottom-right{right:16px;bottom:16px}
.cr-toaster-bottom-center{left:50%;bottom:16px;transform:translateX(-50%)}
.cr-toaster-top-right{right:16px;top:16px}
.cr-toaster-top-center{left:50%;top:16px;transform:translateX(-50%)}
@keyframes cr-toast-in{from{opacity:0;transform:translateY(8px)}}`;
function Toast({ variant = "neutral", title, description, icon, onClose, action, closeLabel = "Dismiss", style, className = "", ...props }) {
  ensureCss("cr-toast-css", css);
  const meta = status(variant);
  const urgent = isUrgent(variant);
  return /* @__PURE__ */ React.createElement("div", { role: urgent ? "alert" : "status", "data-slot": "toast", className: cx("cr-toast", className), style, ...props }, /* @__PURE__ */ React.createElement(Icon, { name: icon || meta.icon, size: 16, color: meta.color, style: { marginTop: 1 } }), /* @__PURE__ */ React.createElement("div", { style: { flex: 1 } }, /* @__PURE__ */ React.createElement("p", { className: "cr-toast-title" }, title), description && /* @__PURE__ */ React.createElement("p", { className: "cr-toast-desc" }, description)), action, onClose && /* @__PURE__ */ React.createElement(IconButton, { icon: "x", label: closeLabel, size: "sm", onClick: onClose }));
}
let seq = 0;
let items = [];
const subs = /* @__PURE__ */ new Set();
const emit = () => subs.forEach((f) => f(items));
function push(variant, title, opts = {}) {
  var _a;
  const id = (_a = opts.id) != null ? _a : ++seq;
  items = [...items.filter((t) => t.id !== id), { ...opts, id, variant, title }];
  emit();
  return id;
}
function toast(title, opts) {
  return push(opts && opts.variant || "neutral", title, opts);
}
toast.success = (t, o) => push("success", t, o);
toast.error = (t, o) => push("destructive", t, o);
toast.warning = (t, o) => push("warning", t, o);
toast.info = (t, o) => push("info", t, o);
toast.dismiss = (id) => {
  items = id == null ? [] : items.filter((t) => t.id !== id);
  emit();
};
function Toaster({ position = "bottom-right", duration = 4e3, className = "", style }) {
  ensureCss("cr-toast-css", css);
  const [list, setList] = React.useState(items);
  React.useEffect(() => {
    subs.add(setList);
    return () => {
      subs.delete(setList);
    };
  }, []);
  React.useEffect(() => {
    const timers = list.filter((t) => {
      var _a;
      return ((_a = t.duration) != null ? _a : duration) !== Infinity;
    }).map((t) => {
      var _a;
      return setTimeout(() => toast.dismiss(t.id), (_a = t.duration) != null ? _a : duration);
    });
    return () => timers.forEach(clearTimeout);
  }, [list, duration]);
  return /* @__PURE__ */ React.createElement("section", { "aria-label": "Notifications", "data-slot": "toaster", className: cx("cr-toaster", `cr-toaster-${position}`, className), style }, list.map((t) => /* @__PURE__ */ React.createElement(Toast, { key: t.id, variant: t.variant, title: t.title, description: t.description, icon: t.icon, action: t.action, onClose: () => toast.dismiss(t.id) })));
}
Object.assign(__ds_scope, { Toast, toast, Toaster });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/Toast.jsx", error: String((e && e.message) || e) }); }

// components/overlay/Tooltip.jsx
try { (() => {
const { ensureCss, cx } = __ds_scope;
const css = `.cr-tooltip-wrap{position:relative;display:inline-flex}
.cr-tooltip{position:absolute;z-index:110;padding:4px 8px;border-radius:var(--radius-md);background:var(--popover);color:var(--popover-foreground);border:1px solid var(--border);box-shadow:var(--shadow-md);font-family:var(--font-sans);font-size:var(--text-xs);font-weight:var(--weight-medium);white-space:nowrap;pointer-events:none;opacity:0;visibility:hidden;transition:opacity var(--duration-fast) var(--ease-out)}
.cr-tooltip-visible{opacity:1;visibility:visible}
.cr-tooltip-top{bottom:calc(100% + 6px);left:50%;transform:translateX(-50%)}
.cr-tooltip-bottom{top:calc(100% + 6px);left:50%;transform:translateX(-50%)}
.cr-tooltip-right{left:calc(100% + 6px);top:50%;transform:translateY(-50%)}
.cr-tooltip-left{right:calc(100% + 6px);top:50%;transform:translateY(-50%)}
.cr-tooltip-kbd{margin-left:6px;color:var(--muted-foreground);font-family:var(--font-mono);font-size:10px}`;
function Tooltip({ content, shortcut, side = "top", delayDuration = 300, open, defaultOpen = false, onOpenChange, children, style, className = "" }) {
  ensureCss("cr-tooltip-css", css);
  const [inner, setInner] = React.useState(defaultOpen);
  const shown = open !== void 0 ? open : inner;
  const timer = React.useRef();
  const ids = React.useId ? React.useId() : "tip";
  const set = (v) => {
    clearTimeout(timer.current);
    if (open === void 0) setInner(v);
    onOpenChange && onOpenChange(v);
  };
  const later = () => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => set(true), delayDuration);
  };
  React.useEffect(() => () => clearTimeout(timer.current), []);
  const child = React.isValidElement(children) ? React.cloneElement(children, { "aria-describedby": ids }) : children;
  return /* @__PURE__ */ React.createElement("span", { "data-slot": "tooltip", className: cx("cr-tooltip-wrap", className), style, onMouseEnter: later, onMouseLeave: () => set(false), onFocus: () => set(true), onBlur: () => set(false), onKeyDown: (e) => {
    if (e.key === "Escape") set(false);
  } }, child, /* @__PURE__ */ React.createElement("span", { id: ids, role: "tooltip", "data-slot": "tooltip-content", className: cx("cr-tooltip", `cr-tooltip-${side}`, shown && "cr-tooltip-visible") }, content, shortcut && /* @__PURE__ */ React.createElement("span", { className: "cr-tooltip-kbd" }, shortcut)));
}
Object.assign(__ds_scope, { Tooltip });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/Tooltip.jsx", error: String((e && e.message) || e) }); }

// components/overlay/ShortcutsDialog.jsx
try { (() => {
const { Dialog, Kbd, ensureCss } = __ds_scope;
const css = `.cr-shortcuts{display:grid;grid-template-columns:1fr 1fr;gap:18px 28px}
.cr-shortcuts-group{min-width:0}
.cr-shortcuts-title{font-size:var(--text-xs);font-weight:var(--weight-semibold);letter-spacing:var(--tracking-caps);text-transform:uppercase;color:var(--muted-foreground);margin:0 0 6px}
.cr-shortcuts-row{display:flex;align-items:center;gap:12px;min-height:26px;font-size:var(--text-sm)}
.cr-shortcuts-row span{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
@media (max-width:560px){.cr-shortcuts{grid-template-columns:1fr}}`;
function ShortcutsDialog({ open = false, onOpenChange, groups = [], title = "Keyboard shortcuts", hotkey = "?", ...props }) {
  ensureCss("cr-shortcuts-css", css);
  React.useEffect(() => {
    if (!hotkey || !onOpenChange) return;
    const onKey = (e) => {
      const t = e.target, tag = t && t.tagName;
      if (t && (t.isContentEditable || tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT")) return;
      if (e.key === hotkey) {
        e.preventDefault();
        onOpenChange(true);
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [hotkey, onOpenChange]);
  return /* @__PURE__ */ React.createElement(Dialog, { open, onOpenChange, title, icon: "keyboard", ...props }, /* @__PURE__ */ React.createElement("div", { className: "cr-shortcuts" }, groups.map((g) => /* @__PURE__ */ React.createElement("div", { key: g.name, className: "cr-shortcuts-group" }, /* @__PURE__ */ React.createElement("p", { className: "cr-shortcuts-title" }, g.name), (g.items || []).map((it) => /* @__PURE__ */ React.createElement("div", { key: it.label, className: "cr-shortcuts-row" }, /* @__PURE__ */ React.createElement("span", null, it.label), /* @__PURE__ */ React.createElement(Kbd, { keys: Array.isArray(it.keys) ? it.keys : [it.keys] })))))));
}
Object.assign(__ds_scope, { ShortcutsDialog });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/overlay/ShortcutsDialog.jsx", error: String((e && e.message) || e) }); }

__ds_ns.Button = __ds_scope.Button;
__ds_ns.IconButton = __ds_scope.IconButton;
__ds_ns.Icon = __ds_scope.Icon;
__ds_ns.Badge = __ds_scope.Badge;
__ds_ns.Avatar = __ds_scope.Avatar;
__ds_ns.Card = __ds_scope.Card;
__ds_ns.Tag = __ds_scope.Tag;
__ds_ns.Kbd = __ds_scope.Kbd;
__ds_ns.Alert = __ds_scope.Alert;
__ds_ns.EmptyState = __ds_scope.EmptyState;
__ds_ns.SelectionBar = __ds_scope.SelectionBar;
__ds_ns.FilterBar = __ds_scope.FilterBar;
__ds_ns.Separator = __ds_scope.Separator;
__ds_ns.Skeleton = __ds_scope.Skeleton;
__ds_ns.Progress = __ds_scope.Progress;
__ds_ns.Table = __ds_scope.Table;
__ds_ns.Checkbox = __ds_scope.Checkbox;
__ds_ns.Input = __ds_scope.Input;
__ds_ns.Textarea = __ds_scope.Textarea;
__ds_ns.Radio = __ds_scope.Radio;
__ds_ns.SegmentedControl = __ds_scope.SegmentedControl;
__ds_ns.RadioCard = __ds_scope.RadioCard;
__ds_ns.Select = __ds_scope.Select;
__ds_ns.Switch = __ds_scope.Switch;
__ds_ns.Tabs = __ds_scope.Tabs;
__ds_ns.Pagination = __ds_scope.Pagination;
__ds_ns.Sidebar = __ds_scope.Sidebar;
__ds_ns.List = __ds_scope.List;
__ds_ns.Popover = __ds_scope.Popover;
__ds_ns.DropdownMenu = __ds_scope.DropdownMenu;
__ds_ns.CommandPalette = __ds_scope.CommandPalette;
__ds_ns.BottomSheet = __ds_scope.BottomSheet;
__ds_ns.Dialog = __ds_scope.Dialog;
__ds_ns.Toast = __ds_scope.Toast;
__ds_ns.Tooltip = __ds_scope.Tooltip;
__ds_ns.ShortcutsDialog = __ds_scope.ShortcutsDialog;
__ds_ns.buttonVariants = __ds_scope.buttonVariants;
__ds_ns.STATUS = __ds_scope.STATUS;
__ds_ns.AlertTitle = __ds_scope.AlertTitle;
__ds_ns.AlertDescription = __ds_scope.AlertDescription;
__ds_ns.TableHeader = __ds_scope.TableHeader;
__ds_ns.TableBody = __ds_scope.TableBody;
__ds_ns.TableFooter = __ds_scope.TableFooter;
__ds_ns.TableRow = __ds_scope.TableRow;
__ds_ns.TableHead = __ds_scope.TableHead;
__ds_ns.TableCell = __ds_scope.TableCell;
__ds_ns.TableCaption = __ds_scope.TableCaption;
__ds_ns.PopoverTrigger = __ds_scope.PopoverTrigger;
__ds_ns.PopoverContent = __ds_scope.PopoverContent;
__ds_ns.DropdownMenuTrigger = __ds_scope.DropdownMenuTrigger;
__ds_ns.DropdownMenuContent = __ds_scope.DropdownMenuContent;
__ds_ns.DropdownMenuItem = __ds_scope.DropdownMenuItem;
__ds_ns.DropdownMenuLabel = __ds_scope.DropdownMenuLabel;
__ds_ns.DropdownMenuSeparator = __ds_scope.DropdownMenuSeparator;
__ds_ns.DropdownMenuCheckboxItem = __ds_scope.DropdownMenuCheckboxItem;
__ds_ns.SidebarSection = __ds_scope.SidebarSection;
__ds_ns.SidebarItem = __ds_scope.SidebarItem;
__ds_ns.ListItem = __ds_scope.ListItem;
__ds_ns.AvatarImage = __ds_scope.AvatarImage;
__ds_ns.AvatarFallback = __ds_scope.AvatarFallback;
__ds_ns.CardHeader = __ds_scope.CardHeader;
__ds_ns.CardTitle = __ds_scope.CardTitle;
__ds_ns.CardDescription = __ds_scope.CardDescription;
__ds_ns.CardAction = __ds_scope.CardAction;
__ds_ns.CardContent = __ds_scope.CardContent;
__ds_ns.CardFooter = __ds_scope.CardFooter;
__ds_ns.Label = __ds_scope.Label;
__ds_ns.RadioGroup = __ds_scope.RadioGroup;
__ds_ns.RadioGroupItem = __ds_scope.RadioGroupItem;
__ds_ns.RadioCardGroup = __ds_scope.RadioCardGroup;
__ds_ns.TabsList = __ds_scope.TabsList;
__ds_ns.TabsTrigger = __ds_scope.TabsTrigger;
__ds_ns.TabsContent = __ds_scope.TabsContent;
__ds_ns.Toaster = __ds_scope.Toaster;
__ds_ns.toast = __ds_scope.toast;

/* The system was published as KoguDesignSystem_6c0df3 before it took the brand's name; keep the old global working for code written against it. */
window.KoguDesignSystem_6c0df3 = __ds_ns;

})();
