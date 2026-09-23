slint::slint! {
    import { ComboBox, LineEdit, ScrollView, HorizontalBox, Palette, AboutSlint } from "std-widgets.slint";
    component MenuButton inherits Rectangle {
        in property <string> text;
        in property <bool> enabled: true;
        in property <bool> primary: false;
        callback clicked();
        min-width: 84px; min-height: 38px;
        background: !enabled ? #49342d : primary ? (hit.pressed ? #e75d08 : hit.has-hover ? #ff8d2b : #ff7515) : (hit.pressed ? #6c3928 : hit.has-hover ? #48251d : #2c1b16);
        border-width: primary ? 0px : 1px;
        border-color: #75402c;
        border-radius: 3px;
        Text { text: parent.text; color: !parent.enabled ? #ac9588 : parent.primary ? #190d09 : #f7e9dc; font-weight: 650; horizontal-alignment: center; vertical-alignment: center; }
        hit := TouchArea { enabled: parent.enabled; clicked => { parent.clicked(); } }
    }
    component ModToggle inherits Rectangle {
        in-out property <bool> checked;
        in property <bool> enabled: true;
        callback toggled();
        width: 24px; height: 24px;
        border-width: 2px; border-color: checked ? #ff7515 : #ae8a77;
        background: checked ? #ff7515 : #21130f;
        border-radius: 2px;
        Text { text: "✓"; color: #1a0e0a; font-size: 18px; font-weight: 800; visible: parent.checked; horizontal-alignment: center; vertical-alignment: center; }
        TouchArea { enabled: parent.enabled; clicked => {parent.checked=!parent.checked; parent.toggled();} }
    }
    export struct ModRow { id: string, name: string, version: string, description: string, checked: bool }
    export component Manager inherits Window {
        init => { Palette.color-scheme = ColorScheme.dark; }
        title: "Deathloop Mod Manager";
        icon: @image-url("../assets/manager-icon.png");
        preferred-width: 860px; preferred-height: 760px;
        min-width: 720px; min-height: 700px;
        background: #24130f;
        default-font-family: "Segoe UI"; default-font-size: 14px;
        in property <string> game-path;
        in property <[string]> profiles;
        in-out property <int> profile-index;
        in property <[ModRow]> mods;
        in property <string> status-text: "Choose your mods, then launch the game.";
        in property <string> mode-text;
        in property <bool> busy;
        in-out property <bool> editing: false;
        in-out property <bool> tools-open: false;
        in-out property <bool> about-open: false;
        in-out property <string> profile-name;
        callback browse(); callback install(); callback launch();
        callback choose-profile(int); callback toggle-mod(string, bool);
        callback profile-action(string, string); callback tool(string); callback remove-mod(string);
        VerticalLayout {
            padding: 28px; spacing: 18px;
            HorizontalLayout {
                VerticalLayout {
                    Text { text: "DEATHLOOP"; color: #ff7515; font-size: 28px; font-weight: 800; }
                    Text { text: "MOD MANAGER"; color: #f5e7d9; font-size: 12px; font-weight: 700; }
                }
                Rectangle { horizontal-stretch: 1; }
                MenuButton { text: "Tools"; enabled: !root.busy; clicked => { root.tools-open = !root.tools-open; } }
            }
            if root.tools-open: HorizontalBox {
                MenuButton { text: "Logs"; clicked => {root.tool("logs");root.tools-open=false;} }
                MenuButton { text: "Mod folder"; clicked => {root.tool("mods");root.tools-open=false;} }
                MenuButton { text: "Help"; clicked => {root.tool("help");root.tools-open=false;} }
                MenuButton { text: "About"; clicked => {root.about-open=!root.about-open;root.tools-open=false;} }
            }
            if root.about-open: Rectangle {
                background: #301a14; border-radius: 4px;
                VerticalLayout { padding: 12px; spacing: 8px;
                    Text { text: "Deathloop Mod Manager 0.3.6"; color: #f7e9dc; }
                    AboutSlint {}
                    MenuButton {text:"Close about";clicked=>{root.about-open=false;}}
                }
            }
            Rectangle {
                background: #301a14; border-radius: 4px;
                VerticalLayout { padding: 16px; spacing: 8px;
                    Text { text: "GAME LOCATION"; color: #e7b699; font-size: 11px; font-weight: 700; }
                    HorizontalLayout { spacing: 12px;
                        Text { text: root.game-path; color: #f7e9dc; overflow: elide; vertical-alignment: center; horizontal-stretch: 1; }
                        MenuButton { text: "Browse…"; enabled: !root.busy; clicked => {root.browse();} }
                    }
                }
            }
            HorizontalLayout { spacing: 12px;
                Text { text: "Profile"; color: #e7b699; vertical-alignment: center; }
                ComboBox { model: root.profiles; current-index <=> root.profile-index; enabled: !root.busy; horizontal-stretch: 1;
                    selected(value) => {root.choose-profile(self.current-index);} }
                MenuButton { text: root.editing ? "Done" : "Edit profiles"; enabled: !root.busy; clicked => {root.editing=!root.editing;} }
            }
            if root.editing: Rectangle {
                background: #301a14; border-radius: 4px;
                VerticalLayout { padding: 14px; spacing: 10px;
                    LineEdit { text <=> root.profile-name; placeholder-text: "Profile name"; enabled: !root.busy; }
                    HorizontalBox {
                        MenuButton { text: "New"; enabled: !root.busy; clicked => {root.profile-action("new",root.profile-name);} }
                        MenuButton { text: "Rename"; enabled: !root.busy; clicked => {root.profile-action("rename",root.profile-name);} }
                        MenuButton { text: "Duplicate"; enabled: !root.busy; clicked => {root.profile-action("duplicate",root.profile-name);} }
                        MenuButton { text: "Delete"; enabled: !root.busy; clicked => {root.profile-action("delete",root.profile-name);} }
                    }
                    Text { text: "Names are local labels. Checkboxes below edit this profile."; color: #cbb6a8; font-size: 12px; }
                }
            }
            Text { text: "INSTALLED MODS"; color: #e7b699; font-size: 11px; font-weight: 700; }
            ScrollView { vertical-stretch: 1;
                VerticalLayout { spacing: 8px;
                    for mod in root.mods: Rectangle {
                        background: mod.checked ? #503024 : #301a14; border-radius: 4px;
                        min-height: 90px;
                        HorizontalLayout { padding: 14px; spacing: 14px;
                            ModToggle { checked: mod.checked; enabled: !root.busy; toggled => {root.toggle-mod(mod.id,self.checked);} }
                            VerticalLayout { spacing: 5px; horizontal-stretch: 1;
                                Text { text: mod.name + "  ·  " + mod.version; color: #f7e9dc; font-weight: 600; }
                                Text { text: mod.description; color: #cbb6a8; wrap: word-wrap; font-size: 12px; }
                            }
                            MenuButton { text: "Remove"; enabled: !root.busy; clicked => {root.remove-mod(mod.id);} }
                        }
                    }
                    if root.mods.length == 0: Text {text:"No mods installed. Install a mod ZIP to get started.";color:#cbb6a8;min-height:90px;wrap:word-wrap;}
                    Rectangle {vertical-stretch:1;}
                }
            }
            Rectangle {height:1px;background:#75402c;}
            Text {text:root.mode-text;color:#ff7515;font-weight:600;}
            Text {text:root.status-text;color:#dfc9ba;wrap:word-wrap;min-height:36px;}
            HorizontalLayout {spacing:12px;
                MenuButton {text:"Install mod…";enabled:!root.busy;clicked=>{root.install();}}
                Rectangle {horizontal-stretch:1;}
                MenuButton {text:root.busy ? "Launching…" : "Launch game";enabled:!root.busy;primary:true;clicked=>{root.launch();}}
            }
        }
    }
}
