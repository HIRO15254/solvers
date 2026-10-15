/** Desktop navigation column (shadcn's Sidebar in Cryst form): Sidebar + SidebarSection + SidebarItem. */
export interface SidebarProps extends React.HTMLAttributes<HTMLElement>{
/** icon-only, `sidebar-collapsed-width` wide */
collapsed?:boolean;
/** product mark and name */
header?:React.ReactNode;
/** pinned to the bottom, e.g. Settings */
footer?:React.ReactNode;
/** accessible name of the nav landmark @default 'Main' */
label?:string;
}
export interface SidebarSectionProps{label?:string;children?:React.ReactNode;}
export interface SidebarItemProps extends React.HTMLAttributes<HTMLElement>{
/** Tabler icon name */
icon?:string;
/** trailing count, e.g. unread */
count?:number|string;
active?:boolean;
/** render an <a> instead of a <button> */
href?:string;
}
