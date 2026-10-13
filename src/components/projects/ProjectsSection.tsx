import React, { useEffect } from "react";
import { useProjectStore } from "@/stores/projectStore";
import { ProjectList } from "./ProjectList";
import { ProjectDetail } from "./ProjectDetail";

export const ProjectsSection: React.FC = () => {
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const loadProjects = useProjectStore((s) => s.loadProjects);
  const subscribe = useProjectStore((s) => s.subscribe);

  useEffect(() => {
    loadProjects();
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadProjects, subscribe]);

  return activeProjectId === null ? <ProjectList /> : <ProjectDetail />;
};
