/** Tabler icon glyph from the self-hosted webfont. The only sanctioned way to render icons in Cryst (shadcn uses lucide; Cryst swaps in Tabler). */
export interface IconProps extends React.HTMLAttributes<HTMLElement>{
/** Tabler icon name without the `ti-` prefix, e.g. "settings", "circle-check" */
name:string;
/** px size @default 16 */
size?:number;
/** CSS color; defaults to currentColor */
color?:string;
/** use the -filled variant @default false */
filled?:boolean;
}
