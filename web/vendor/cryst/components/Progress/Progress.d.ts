/** Progress bar (shadcn Progress). value={null} renders the indeterminate state. */
export interface ProgressProps extends React.HTMLAttributes<HTMLDivElement>{
/** 0–max, or null when the length is unknown @default 0 */
value?:number|null;
/** @default 100 */
max?:number;
/** @default 'default' */
variant?:'default'|'success'|'warning'|'destructive';
/** bar height: sm 4 / md 6 / lg 10 @default 'md' */
size?:'sm'|'md'|'lg';
/** accessible name, e.g. "Upload" */
label?:string;
}
