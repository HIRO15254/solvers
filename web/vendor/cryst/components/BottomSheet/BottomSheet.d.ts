/** Mobile sheet (shadcn Drawer), in two kinds. Dismissible (default without onConfirm): grab + centered title; drag, scrim tap or Esc closes it. Explicit (onConfirm, or dismissible={false}): no grab; × cancel top-left, ✓ confirm top-right; nothing else closes it. */
export interface BottomSheetProps extends React.HTMLAttributes<HTMLDivElement>{
open:boolean;
/** called with false when the sheet closes (grab drag, scrim tap, Esc, or × without onCancel) */
onOpenChange?:(open:boolean)=>void;
title?:string;
/** force the kind; defaults to true without onConfirm and false with it */
dismissible?:boolean;
/** explicit sheets: replaces the default close on ×, e.g. to confirm discarding changes */
onCancel?:()=>void;
/** explicit sheets: the ✓ action */
onConfirm?:()=>void;
/** accessible name of × @default 'Cancel' */
cancelLabel?:string;
/** accessible name of ✓ @default 'Save' */
confirmLabel?:string;
confirmDisabled?:boolean;
/** ✓ shows a spinner, is disabled and sets aria-busy */
confirmLoading?:boolean;
}
