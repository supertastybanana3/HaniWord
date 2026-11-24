import { Bot, Settings, Megaphone, BookA, Palette } from "lucide-react"

import {
    Sidebar,
    SidebarContent,
    SidebarFooter,
    SidebarGroup,
    SidebarGroupContent,
    SidebarGroupLabel,
    SidebarMenu,
    SidebarMenuButton,
    SidebarMenuItem,
} from "@/components/ui/sidebar"

// Menu items.
const contents = [
    {
        title: "全般",
        url: "/",
        icon: Settings,
    },
    // 2026-11-06 (仕事 37): 候補の窓の外観 (フォント・大きさ・色・並べ方・足りない字の補い)
    {
        title: "外観",
        url: "/appearance",
        icon: Palette,
    },
    {
        title: "Zenzai",
        url: "/zenzai",
        icon: Bot,
    },
    {
        title: "単語登録",
        url: "/user-dictionary",
        icon: BookA,
    },
]

// Footer items.
const footer = [
    {
        title: "HaniWord について",
        url: "/about",
        icon: Megaphone,
    },
]

export function AppSidebar() {
    let currentPath = window.location.pathname;

    return (
        <Sidebar>
            <SidebarContent>
                <SidebarGroup>
                    <SidebarGroupLabel>設定</SidebarGroupLabel>
                    <SidebarGroupContent>
                        <SidebarMenu>
                            {contents.map((item) => (
                                <SidebarMenuItem key={item.title} className={currentPath == item.url ? "[&>*]:bg-sidebar-accent" : ""}>
                                    <SidebarMenuButton asChild>
                                        <a href={item.url}>
                                            <item.icon />
                                            <span>{item.title}</span>
                                        </a>
                                    </SidebarMenuButton>
                                </SidebarMenuItem>
                            ))}
                        </SidebarMenu>
                    </SidebarGroupContent>
                </SidebarGroup>
            </SidebarContent>
            <SidebarFooter>
                <SidebarGroup>
                    <SidebarGroupContent>
                        <SidebarMenu>
                            {footer.map((item) => (
                                <SidebarMenuItem key={item.title} className={currentPath == item.url ? "[&>*]:bg-sidebar-accent" : ""}>
                                    <SidebarMenuButton asChild>
                                        <a href={item.url}>
                                            <item.icon />
                                            <span>{item.title}</span>
                                        </a>
                                    </SidebarMenuButton>
                                </SidebarMenuItem>
                            ))}
                        </SidebarMenu>
                    </SidebarGroupContent>
                </SidebarGroup>
            </SidebarFooter>
        </Sidebar>
    )
}
