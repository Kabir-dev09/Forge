#include "step_progress.h"
#include "main_window.h"
#include "../core/network_client.h"
#include "../core/package_manager.h"
#include "../core/extractor.h"
#include "../core/installer_state.h"
#include "../utils/system_utils.h"
#include <FL/Fl.H>

namespace gui {

StepProgress::StepProgress(int x, int y, int w, int h, MainWindow* parent_window, core::InstallerState& state)
    : Fl_Group(x, y, w, h), main_window(parent_window), install_state(state), is_running(true)
{
    Fl_Box* title = new Fl_Box(x, y + 20, w, 50, "Installing Forge");
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
    progress_bar->selection_color(fl_rgb_color(0, 120, 215));
    
    btn_finish = new Fl_Button(x + (w - 150) / 2, y + 220, 150, 40, "Finish");
    btn_finish->callback(cb_finish, this);
    btn_finish->box(FL_FLAT_BOX);
    btn_finish->color(fl_rgb_color(0, 120, 215));
    btn_finish->labelcolor(FL_WHITE);
    btn_finish->hide();
    
    end();
    
    // Start background thread
    worker_thread = std::thread(&StepProgress::run_installation, this);
}

StepProgress::~StepProgress() {
    if (worker_thread.joinable()) {
        worker_thread.join();
    }
}

void StepProgress::set_status(const std::string& msg, float progress) {
    ProgressMessage* p = new ProgressMessage{this, msg, progress, false, false};
    Fl::awake(update_ui_cb, p);
}

void StepProgress::finish_with_error(const std::string& err) {
    ProgressMessage* p = new ProgressMessage{this, err, 0.0f, true, true};
    Fl::awake(update_ui_cb, p);
    is_running = false;
}

void StepProgress::finish_with_success() {
    ProgressMessage* p = new ProgressMessage{this, "Installation complete!", 100.0f, false, true};
    Fl::awake(update_ui_cb, p);
    is_running = false;
}

void StepProgress::update_ui_cb(void* data) {
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
        p->self->status_label->labelcolor(fl_rgb_color(40, 167, 69)); // Green
        p->self->progress_bar->value(p->progress);
        p->self->btn_finish->show();
    } else {
        p->self->status_label->copy_label(p->text.c_str());
        p->self->progress_bar->value(p->progress);
    }
    
    p->self->redraw();
    delete p;
}

void StepProgress::run_installation() {
    try {
        set_status("Fetching latest release info...", 10.0f);
        core::ReleaseInfo release = core::NetworkClient::get_latest_release();
        
        if (release.binary_url.empty()) {
            finish_with_error("Could not find binary download URL.");
            return;
        }

        std::string tmp_tar = "/tmp/forge-binary.tar.gz";
        set_status("Downloading " + release.version_tag + "...", 30.0f);
        if (!core::NetworkClient::download_file(release.binary_url, tmp_tar)) {
            finish_with_error("Download failed.");
            return;
        }

        set_status("Installing system dependencies...", 60.0f);
        if (core::PackageManager::detect_package_manager() != core::PackageManagerType::UNKNOWN) {
            std::vector<std::string> deps = core::PackageManager::get_runtime_dependencies();
            if (!core::PackageManager::install_dependencies(deps, install_state.sudo_password)) {
                finish_with_error("Failed to install system dependencies.");
                return;
            }
        }

        set_status("Extracting and installing binary...", 80.0f);
        bool is_global = (install_state.scope == core::InstallScope::GLOBAL);
        if (!core::Extractor::extract_and_install_binary(
                tmp_tar, release.version_tag, is_global, install_state.sudo_password)) {
            finish_with_error("Extraction failed.");
            return;
        }

        set_status("Saving state...", 95.0f);
        install_state.installed_version = release.version_tag;
        install_state.is_installed = true;
        if (!core::StateManager::save_state(install_state, install_state.sudo_password)) {
            finish_with_error("Failed to save installation state.");
            return;
        }

        finish_with_success();
        
    } catch (const std::exception& e) {
        finish_with_error(e.what());
    }
}

void StepProgress::cb_finish(Fl_Widget*, void* data) {
    StepProgress* self = static_cast<StepProgress*>(data);
    self->main_window->hide(); // Close app
}

} // namespace gui
