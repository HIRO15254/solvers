/** Modal dialog for desktop. shadcn-style open/onOpenChange; Esc and scrim click close; focus moves in and returns. On mobile use BottomSheet. */
export interface DialogProps extends React.HTMLAttributes<HTMLDivElement>{
open:boolean;
onOpenChange?:(open:boolean)=>void;
/** legacy close callback */
onClose?:()=>void;
title?:string;
description?:string;
/** Tabler icon before the title */
icon?:string;
/** right-aligned footer actions */
footer?:React.ReactNode;
/** @default 'Close' */
closeLabel?:string;
}
