/** Inline message that belongs to the page and stays until the situation changes (shadcn Alert), with AlertTitle and AlertDescription parts. Same status vocabulary as Toast and Badge. */
export interface AlertProps extends React.HTMLAttributes<HTMLDivElement>{
/** 'default' is an alias of 'neutral' @default 'neutral' */
variant?:'neutral'|'default'|'info'|'success'|'warning'|'destructive';
/** override the variant icon, or null for none */
icon?:string|null;
title?:React.ReactNode;
description?:React.ReactNode;
/** buttons under the text */
actions?:React.ReactNode;
}
