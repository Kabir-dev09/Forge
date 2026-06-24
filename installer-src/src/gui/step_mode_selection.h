#pragma once
#include <FL/Fl_Group.H>
#include <FL/Fl_Round_Button.H>
#include "../core/installer_state.h"

namespace gui {

class MainWindow;

class StepModeSelection : public Fl_Group {
public:
    StepModeSelection(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state);
    
private:
    MainWindow* main_window;
    
    Fl_Round_Button* rb_global;
    Fl_Round_Button* rb_user;
    
    static void cb_next(Fl_Widget* w, void* data);
    static void cb_back(Fl_Widget* w, void* data);
};

} // namespace gui
