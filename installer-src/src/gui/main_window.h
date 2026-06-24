#pragma once
#include <FL/Fl_Double_Window.H>
#include <FL/Fl_Group.H>
#include <FL/Fl_Button.H>
#include "../core/installer_state.h"

namespace gui {

class MainWindow : public Fl_Double_Window {
public:
    MainWindow();
    ~MainWindow();

    void show_welcome_step();
    void show_mode_selection_step();
    void show_progress_step();
    void show_uninstall_confirm_step();
    void show_uninstall_progress_step();
    
    core::InstallerState& get_state() { return current_state; }

private:
    core::InstallerState current_state;
    Fl_Group* current_step_group;
    
    Fl_Button* btn_cancel;

    void clear_current_step();
    static void cb_cancel(Fl_Widget* w, void* data);
};

} // namespace gui
