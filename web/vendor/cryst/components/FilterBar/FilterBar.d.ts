/** The filters currently narrowing a list. */
export interface FilterBarProps extends React.HTMLAttributes<HTMLDivElement>{
/** {id, label, field?, icon?} — `field` is the name, `label` the value */
filters?:Array<{id?:string;label:string;field?:string;icon?:string}>;
onRemove?:(id:string)=>void;
/** shown once more than one filter is set */
onClear?:()=>void;
/** @default 'Clear all' */
clearLabel?:string;
/** @default 'Remove filter' */
removeLabel?:string;
}
