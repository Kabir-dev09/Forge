#pragma once
#include <FL/Fl_Group.H>
#include <FL/Fl_Secret_Input.H>
#include "../core/installer_state.h"

namespace gui {

class MainWindow;

class StepWelcome : public Fl_Group {
public:
    StepWelcome(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state);
    
private:
    MainWindow* main_window;
    Fl_Secret_Input* input_sudo;
    
    bool validate_and_store_password();
    
    static void cb_install(Fl_Widget* w, void* data);
    static void cb_upgrade(Fl_Widget* w, void* data);
    static void cb_uninstall(Fl_Widget* w, void* data);
};

} // namespace gui
