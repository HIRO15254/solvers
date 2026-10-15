/** Notification card, plus a Sonner-style API: mount <Toaster/> once, then call toast('Saved'), toast.success(), toast.error(), toast.warning(), toast.info(), toast.dismiss(id). */
export interface ToastProps extends React.HTMLAttributes<HTMLDivElement>{
/** the system's status vocabulary, shared with Alert and Badge @default 'neutral' */
variant?:'neutral'|'default'|'info'|'success'|'warning'|'destructive';
title:string;
description?:string;
/** override the variant's default Tabler icon */
icon?:string;
/** inline action, usually a ghost Button */
action?:React.ReactNode;
onClose?:()=>void;
/** @default 'Dismiss' */
closeLabel?:string;
}
export interface ToasterProps{
/** @default 'bottom-right'; use 'bottom-center' on mobile */
position?:'bottom-right'|'bottom-center'|'top-right'|'top-center';
/** ms; Infinity keeps toasts until dismissed @default 4000 */
duration?:number;
className?:string;
style?:React.CSSProperties;
}
export interface ToastOptions{id?:string|number;description?:string;icon?:string;action?:React.ReactNode;duration?:number;variant?:ToastProps['variant'];}
