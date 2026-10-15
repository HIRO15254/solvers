/** The keyboard shortcut sheet, opened with `?`. */
export interface ShortcutsDialogProps{
open?:boolean;
onOpenChange?:(open:boolean)=>void;
/** [{name, items:[{label, keys}]}] — keys as a string or one string per cap */
groups?:Array<{name:string;items:Array<{label:string;keys:string|string[]}>}>;
/** @default 'Keyboard shortcuts' */
title?:string;
/** key that opens it when no field has focus; null turns it off @default '?' */
hotkey?:string|null;
}
