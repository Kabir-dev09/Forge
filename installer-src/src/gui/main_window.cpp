#include "main_window.h"
#include "step_welcome.h"
#include "step_mode_selection.h"
#include "step_progress.h"
#include "step_uninstall_confirm.h"
#include "step_uninstall_progress.h"
#include <FL/Fl.H>

namespace gui {

MainWindow::MainWindow() : Fl_Double_Window(600, 450, "Forge Installer"), current_step_group(nullptr) {
    // Read the current system state
    current_state = core::StateManager::get_current_state();
    
    color(fl_rgb_color(250, 250, 250)); // Very light gray/white background for a modern look
    
    // Bottom navigation area (Cancel button)
    btn_cancel = new Fl_Button(480, 400, 100, 30, "Cancel");
    btn_cancel->callback(cb_cancel, this);
    btn_cancel->box(FL_FLAT_BOX);
    btn_cancel->color(fl_rgb_color(220, 220, 220));
    
    show_welcome_step();
    end();
}

MainWindow::~MainWindow() {}

void MainWindow::clear_current_step() {
    if (current_step_group) {
        remove(current_step_group);
        delete current_step_group;
        current_step_group = nullptr;
    }
}

void MainWindow::show_welcome_step() {
    clear_current_step();
    begin();
    current_step_group = new StepWelcome(0, 0, 600, 380, this, current_state);
    end();
    redraw();
}

void MainWindow::show_mode_selection_step() {
    clear_current_step();
    begin();
    current_step_group = new StepModeSelection(0, 0, 600, 380, this, current_state);
    end();
    redraw();
}

void MainWindow::show_progress_step() {
    clear_current_step();
    begin();
    current_step_group = new StepProgress(0, 0, 600, 380, this, current_state);
    end();
    redraw();
}

void MainWindow::show_uninstall_confirm_step() {
    clear_current_step();
    begin();
    current_step_group = new StepUninstallConfirm(0, 0, 600, 380, this, current_state);
    end();
    redraw();
}

void MainWindow::show_uninstall_progress_step() {
    clear_current_step();
    begin();
    current_step_group = new StepUninstallProgress(0, 0, 600, 380, this, current_state);
    end();
    redraw();
}

void MainWindow::cb_cancel(Fl_Widget*, void* data) {
    MainWindow* self = static_cast<MainWindow*>(data);
    self->hide(); // Closes the window and exits
}

} // namespace gui
