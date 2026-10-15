/** Tabs. Short form: items. Composed (shadcn): TabsList, TabsTrigger, TabsContent. Arrow keys, Home and End move between tabs. */
export interface TabsProps extends React.HTMLAttributes<HTMLDivElement>{
/** short form: strings or {id,label,icon?,count?,disabled?} */
items?:Array<string|{id:string;label:string;icon?:string;count?:number;disabled?:boolean}>;
value?:string;
defaultValue?:string;
onValueChange?:(value:string)=>void;
/** legacy alias of onValueChange */
onChange?:(value:string)=>void;
/** @default 'underline' */
variant?:'underline'|'pill';
/** xl = mobile: m-text-secondary labels, m-control-tall tabs @default 'md' */
size?:'md'|'default'|'xl';
/** stretch the bar and split it evenly between tabs (mobile) */
fullWidth?:boolean;
}
export interface TabsListProps extends React.HTMLAttributes<HTMLDivElement>{variant?:'underline'|'pill';size?:'md'|'default'|'xl';fullWidth?:boolean;}
export interface TabsTriggerProps extends React.ButtonHTMLAttributes<HTMLButtonElement>{value:string;icon?:string;count?:number;}
export interface TabsContentProps extends React.HTMLAttributes<HTMLDivElement>{value:string;}
