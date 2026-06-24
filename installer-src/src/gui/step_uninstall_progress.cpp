#include "step_uninstall_progress.h"
#include "main_window.h"
#include "../core/uninstaller.h"
#include <FL/Fl.H>

namespace gui {

StepUninstallProgress::StepUninstallProgress(int x, int y, int w, int h, MainWindow* parent_window, core::InstallerState& state)
    : Fl_Group(x, y, w, h), main_window(parent_window), install_state(state), is_running(true)
{
    Fl_Box* title = new Fl_Box(x, y + 20, w, 50, "Uninstalling Forge");
    title->labelsize(28);
    title->labelfont(FL_HELVETICA_BOLD);

    status_label = new Fl_Box(x, y + 100, w, 30, "Initializing...");
    status_label->labelsize(14);
    status_label->labelfont(FL_HELVETICA);

    progress_bar = new Fl_Progress(x + 50, y + 150, w - 100, 25);
    progress_bar->minimum(0.0);
    progress_bar->maximum(100.0);
    progress_bar->value(0.0);
    progress_bar->color(FL_WHITE);
    progress_bar->selection_color(fl_rgb_color(220, 53, 69)); // Red for uninstall

    btn_finish = new Fl_Button(x + (w - 150) / 2, y + 220, 150, 40, "Close");
    btn_finish->callback(cb_finish, this);
    btn_finish->box(FL_FLAT_BOX);
    btn_finish->color(fl_rgb_color(0, 120, 215));
    btn_finish->labelcolor(FL_WHITE);
    btn_finish->hide();

    end();

    worker_thread = std::thread(&StepUninstallProgress::run_uninstall, this);
}

StepUninstallProgress::~StepUninstallProgress() {
    if (worker_thread.joinable()) {
        worker_thread.join();
    }
}

void StepUninstallProgress::set_status(const std::string& msg, float progress) {
    ProgressMessage* p = new ProgressMessage{this, msg, progress, false, false};
    Fl::awake(update_ui_cb, p);
}

void StepUninstallProgress::finish_with_error(const std::string& err) {
    ProgressMessage* p = new ProgressMessage{this, err, 0.0f, true, true};
    Fl::awake(update_ui_cb, p);
    is_running = false;
}

void StepUninstallProgress::finish_with_success() {
    ProgressMessage* p = new ProgressMessage{this, "Forge has been uninstalled.", 100.0f, false, true};
    Fl::awake(update_ui_cb, p);
    is_running = false;
}

void StepUninstallProgress::update_ui_cb(void* data) {
    ProgressMessage* p = static_cast<ProgressMessage*>(data);
    if (!p) return;

    if (p->is_error) {
        p->self->status_label->copy_label(("Error: " + p->text).c_str());
        p->self->status_label->labelcolor(fl_rgb_color(220, 53, 69));
        p->self->progress_bar->selection_color(fl_rgb_color(220, 53, 69));
        p->self->btn_finish->label("Close");
        p->self->btn_finish->show();
    } else if (p->is_done) {
        p->self->status_label->copy_label(p->text.c_str());
        p->self->status_label->labelcolor(fl_rgb_color(40, 167, 69)); // Green on success
        p->self->progress_bar->value(p->progress);
        p->self->btn_finish->show();
    } else {
        p->self->status_label->copy_label(p->text.c_str());
        p->self->progress_bar->value(p->progress);
    }

    p->self->redraw();
    delete p;
}

void StepUninstallProgress::run_uninstall() {
    set_status("Removing files...", 40.0f);

    bool ok = core::Uninstaller::uninstall(install_state, install_state.sudo_password);

    if (!ok) {
        finish_with_error("Some files could not be removed. Check terminal for details.");
        return;
    }

    set_status("Done.", 100.0f);
    finish_with_success();
}

void StepUninstallProgress::cb_finish(Fl_Widget*, void* data) {
    StepUninstallProgress* self = static_cast<StepUninstallProgress*>(data);
    self->main_window->hide();
}

} // namespace gui
