#include "ange/ecs.hpp"

#include <stdexcept>

namespace ange {

void validate_param(const std::string& name, int value) {
    if (value < 0) {
        throw std::runtime_error("bad value for " + name);
    }
}

EcsWorld::EcsWorld() : entities_() {}

EcsWorld::~EcsWorld() = default;

void EcsWorld::run() {
    validate_param("tick", 1);
}

int EcsWorld::entity_count() const {
    return static_cast<int>(entities_.size());
}

}  // namespace ange
