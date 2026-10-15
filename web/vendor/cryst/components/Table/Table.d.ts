/** Data table (shadcn Table parts): Table, TableHeader, TableBody, TableFooter, TableRow, TableHead, TableCell, TableCaption. */
export interface TableProps extends React.TableHTMLAttributes<HTMLTableElement>{
/** 32px rows instead of 38px, for dense views */
compact?:boolean;
}
export interface TableHeadProps extends React.ThHTMLAttributes<HTMLTableCellElement>{
/** right-align for figures */
numeric?:boolean;
/** makes the header a sort button */
onSort?:()=>void;
/** current sort of this column */
sorted?:'asc'|'desc'|false;
}
export interface TableCellProps extends React.TdHTMLAttributes<HTMLTableCellElement>{
/** right-aligned tabular mono figures */
numeric?:boolean;
}
export interface TableRowProps extends React.HTMLAttributes<HTMLTableRowElement>{selected?:boolean;}
