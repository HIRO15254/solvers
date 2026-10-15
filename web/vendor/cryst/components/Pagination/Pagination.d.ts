/** Page navigation for lists and tables. */
export interface PaginationProps extends React.HTMLAttributes<HTMLElement>{
/** current page, 1-based @default 1 */
page?:number;
/** total number of pages @default 1 */
pageCount?:number;
onPageChange?:(page:number)=>void;
/** pages shown either side of the current one @default 1 */
siblings?:number;
/** xl = 44px touch targets @default 'md' */
size?:'md'|'xl';
/** localized accessible labels: {previous, next, page} */
labels?:{previous?:string;next?:string;page?:string};
}
