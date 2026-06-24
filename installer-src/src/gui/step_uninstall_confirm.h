#pragma once
#include <FL/Fl_Group.H>
#include "../core/installer_state.h"

namespace gui {

class MainWindow;

class StepUninstallConfirm : public Fl_Group {
public:
    StepUninstallConfirm(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state);

private:
    MainWindow* main_window;
    const core::InstallerState& install_state;

    static void cb_confirm(Fl_Widget* w, void* data);
    static void cb_back(Fl_Widget* w, void* data);
};

} // namespace gui
